//! PHP semantic extraction from tree-sitter CSTs.
//!
//! Handles the PHP language family, focusing on Laravel:
//!
//! Extracts:
//! - `Route::get()`, `Route::post()`, etc. route definitions
//! - `Route::prefix()->group()` and `Route::middleware()->group()` context propagation
//! - `->middleware('auth')` auth detection
//! - `Http::get()`, Guzzle HTTP client calls
//! - Log sinks with PII detection

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::patterns;
use crate::tree_sitter::SupportedLanguage;
use crate::types::*;

use super::common::{
    self, anchor_from_node, extract_string_value, node_text, node_text_ref, truncate_call_text,
};

/// Context accumulated from `Route::prefix()->middleware()->group()` chains.
///
/// Pushed onto the group stack when entering a `->group(closure)` call,
/// popped when exiting the closure body.
#[derive(Debug, Clone, Default)]
struct GroupContext {
    prefix: Option<String>,
    auth: Option<AuthKind>,
}

/// Extract semantic information from a PHP source file.
pub fn extract(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
) -> FileExtraction {
    let root = tree.root_node();
    let mut extraction = common::new_extraction(file_path, language);
    let mut group_stack: Vec<GroupContext> = Vec::new();

    extract_recursive(&root, source, file_path, &mut extraction, &mut group_stack);

    extraction
}

fn extract_recursive(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
    group_stack: &mut Vec<GroupContext>,
) {
    // Check for Route::prefix/middleware->...->group(closure) patterns.
    // When detected, we push context onto the stack, walk the closure body,
    // pop the context, and return — skipping normal child traversal to avoid
    // double-processing nodes inside the group closure.
    if try_enter_group(node, source, file_path, extraction, group_stack) {
        return;
    }

    match node.kind() {
        // Laravel routes: Route::get('/path', ...)
        "scoped_call_expression" => {
            try_extract_laravel_route(node, source, file_path, extraction, group_stack);
            try_extract_http_facade_call(node, source, file_path, extraction);
            common::try_extract_log_sink(node, source, file_path, extraction);
        }
        // Method chains: ->middleware('auth')
        "member_call_expression" => {
            common::try_extract_log_sink(node, source, file_path, extraction);
        }
        "function_call_expression" => {
            common::try_extract_log_sink(node, source, file_path, extraction);
        }
        _ => {}
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            extract_recursive(&child, source, file_path, extraction, group_stack);
        }
    }
}

/// Detect `Route::prefix(...)->middleware(...)->group(function() { ... })` chains.
///
/// In tree-sitter PHP, the chain `Route::prefix('/api')->middleware('auth:api')->group(fn)`
/// is parsed as nested call expressions:
///
/// ```text
/// member_call_expression          // ->group(fn)
///   object: member_call_expression  // ->middleware('auth:api')
///     object: scoped_call_expression  // Route::prefix('/api')
///   arguments: (anonymous_function)
/// ```
///
/// This function detects the outermost `->group(closure)` call, walks backwards
/// through the method chain to collect prefix and middleware context, pushes it
/// onto the stack, recursively walks the closure body, then pops the context.
///
/// Returns `true` if this node was a group call (caller should skip normal traversal).
fn try_enter_group(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
    group_stack: &mut Vec<GroupContext>,
) -> bool {
    // We only care about member_call_expression nodes where the method is "group"
    if node.kind() != "member_call_expression" {
        return false;
    }

    let method_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return false,
    };

    if node_text_ref(&method_node, source) != "group" {
        return false;
    }

    // Verify this chain originates from Route:: by walking backwards
    if !chain_starts_with_route(node, source) {
        return false;
    }

    // Collect context from the method chain (prefix, middleware)
    let ctx = collect_chain_context(node, source);

    // Find the closure argument to ->group()
    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return false,
    };

    let closure_body = find_closure_body(&args);
    let closure_body = match closure_body {
        Some(body) => body,
        None => return false,
    };

    // Push context, walk closure, pop context
    group_stack.push(ctx);

    let child_count = closure_body.child_count();
    for i in 0..child_count {
        if let Some(child) = closure_body.child(i as u32) {
            extract_recursive(&child, source, file_path, extraction, group_stack);
        }
    }

    group_stack.pop();

    true
}

/// Walk backwards through a method chain to check if it originates from `Route::`.
fn chain_starts_with_route(node: &Node, source: &str) -> bool {
    let mut current = match node.child_by_field_name("object") {
        Some(obj) => obj,
        None => return false,
    };

    // Walk backwards through nested member_call_expression / scoped_call_expression
    loop {
        match current.kind() {
            "scoped_call_expression" => {
                // Check if scope is "Route"
                if let Some(scope) = current.child_by_field_name("scope") {
                    return node_text_ref(&scope, source) == "Route";
                }
                return false;
            }
            "member_call_expression" => {
                current = match current.child_by_field_name("object") {
                    Some(obj) => obj,
                    None => return false,
                };
            }
            _ => return false,
        }
    }
}

/// Collect prefix and middleware context from the method chain leading to `->group()`.
///
/// Walks backwards from the `->group()` node through the chain collecting:
/// - `prefix("...")` — stored as the prefix
/// - `middleware("...")` — stored as auth if it matches an auth indicator
fn collect_chain_context(group_node: &Node, source: &str) -> GroupContext {
    let mut ctx = GroupContext::default();

    let mut current = match group_node.child_by_field_name("object") {
        Some(obj) => obj,
        None => return ctx,
    };

    loop {
        match current.kind() {
            "member_call_expression" => {
                // Check the method name of this link in the chain
                if let Some(name_node) = current.child_by_field_name("name") {
                    let method_name = node_text_ref(&name_node, source);
                    if let Some(args) = current.child_by_field_name("arguments") {
                        match method_name {
                            "prefix" => {
                                ctx.prefix = find_first_string_arg(&args, source);
                            }
                            "middleware" => {
                                if let Some(mw_name) = find_first_string_arg(&args, source)
                                    && patterns::is_auth_indicator(&mw_name) {
                                        ctx.auth = Some(AuthKind::Middleware(mw_name));
                                    }
                            }
                            _ => {}
                        }
                    }
                }
                // Continue walking backwards
                current = match current.child_by_field_name("object") {
                    Some(obj) => obj,
                    None => break,
                };
            }
            "scoped_call_expression" => {
                // This is the Route::prefix(...) or Route::middleware(...) at the start
                if let Some(name_node) = current.child_by_field_name("name") {
                    let method_name = node_text_ref(&name_node, source);
                    if let Some(args) = current.child_by_field_name("arguments") {
                        match method_name {
                            "prefix" => {
                                ctx.prefix = find_first_string_arg(&args, source);
                            }
                            "middleware" => {
                                if let Some(mw_name) = find_first_string_arg(&args, source)
                                    && patterns::is_auth_indicator(&mw_name) {
                                        ctx.auth = Some(AuthKind::Middleware(mw_name));
                                    }
                            }
                            _ => {}
                        }
                    }
                }
                break;
            }
            _ => break,
        }
    }

    ctx
}

/// Find the closure body (compound_statement) inside the arguments to `->group()`.
///
/// Looks for `anonymous_function` or `arrow_function` nodes in the argument list,
/// then returns their body (`compound_statement`).
fn find_closure_body<'a>(args: &'a Node<'a>) -> Option<Node<'a>> {
    for i in 0..args.named_child_count() {
        if let Some(child) = args.named_child(i as u32) {
            let inner = if child.kind() == "argument" {
                child.named_child(0).unwrap_or(child)
            } else {
                child
            };

            match inner.kind() {
                "anonymous_function" => {
                    // The body of `function() { ... }` is a compound_statement
                    return inner.child_by_field_name("body");
                }
                "arrow_function" => {
                    // Arrow functions: `fn() => expr` — the body is the expression
                    return inner.child_by_field_name("body");
                }
                _ => {}
            }
        }
    }
    None
}

/// Resolve the accumulated auth from the group stack (innermost takes precedence).
fn resolve_group_auth(group_stack: &[GroupContext]) -> Option<AuthKind> {
    // Walk from innermost to outermost — first auth found wins
    for ctx in group_stack.iter().rev() {
        if let Some(ref auth) = ctx.auth {
            return Some(auth.clone());
        }
    }
    None
}

/// Concatenate all prefixes from the group stack with a local route path.
///
/// Avoids double slashes and handles empty prefixes/paths.
fn apply_group_prefix(group_stack: &[GroupContext], local_path: &str) -> String {
    let mut combined = String::new();

    for ctx in group_stack {
        if let Some(ref prefix) = ctx.prefix {
            let trimmed = prefix.trim_end_matches('/');
            if !trimmed.is_empty() {
                if !trimmed.starts_with('/') {
                    combined.push('/');
                }
                combined.push_str(trimmed);
            }
        }
    }

    if combined.is_empty() {
        return local_path.to_string();
    }

    if local_path.is_empty() || local_path == "/" {
        return combined;
    }

    // Ensure exactly one slash between prefix and path
    let local_trimmed = local_path.trim_start_matches('/');
    format!("{combined}/{local_trimmed}")
}

/// Try to extract a Laravel route from `Route::get('/path', ...)`.
///
/// Handles:
/// ```php
/// Route::get('/users', [UserController::class, 'index']);
/// Route::post('/orders', [OrderController::class, 'store'])->middleware('auth');
/// ```
///
/// When called inside a `Route::prefix()->group()`, the accumulated prefix from
/// the group stack is prepended to the route path, and group-level middleware
/// auth is inherited if the route doesn't have its own.
fn try_extract_laravel_route(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
    group_stack: &[GroupContext],
) {
    // Check if this is Route::method(...)
    let scope = match node.child_by_field_name("scope") {
        Some(s) => s,
        None => return,
    };

    let scope_name = node_text_ref(&scope, source);
    if scope_name != "Route" {
        return;
    }

    let method_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };

    let method_name = node_text_ref(&method_node, source);

    // Handle Route::resource() — expands to 7 RESTful routes
    // Handle Route::apiResource() — expands to 5 RESTful routes (no create/edit)
    if method_name == "resource" || method_name == "apiResource" {
        let args = match node.child_by_field_name("arguments") {
            Some(a) => a,
            None => return,
        };
        if let Some(resource_path) = find_first_string_arg(&args, source) {
            let is_api = method_name == "apiResource";
            let local_auth = detect_middleware_chain(node, source);
            let auth = local_auth.or_else(|| resolve_group_auth(group_stack));
            let full_path = apply_group_prefix(group_stack, &resource_path);
            expand_resource_routes(&full_path, is_api, file_path, node, auth, extraction);
        }
        return;
    }

    // Handle Route::any() — matches all HTTP methods
    if method_name == "any" {
        let args = match node.child_by_field_name("arguments") {
            Some(a) => a,
            None => return,
        };
        let route_path = match find_first_string_arg(&args, source) {
            Some(p) => p,
            None => return,
        };
        let local_auth = detect_middleware_chain(node, source);
        let auth = local_auth.or_else(|| resolve_group_auth(group_stack));
        let full_path = apply_group_prefix(group_stack, &route_path);
        extraction.interfaces.push(Interface {
            method: HttpMethod::All,
            path: full_path.clone(),
            auth,
            anchor: anchor_from_node(node, file_path),
            parameters: common::extract_path_params(&full_path),
            handler_name: None,
            request_body_type: None,
        });
        return;
    }

    let http_method = match common::parse_http_method(method_name) {
        Some(m) => m,
        None => return,
    };

    // Extract path from first argument
    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return,
    };

    let route_path = match find_first_string_arg(&args, source) {
        Some(p) => p,
        None => return,
    };

    // Check for ->middleware('auth') chain — local auth takes precedence over group auth
    let local_auth = detect_middleware_chain(node, source);
    let auth = local_auth.or_else(|| resolve_group_auth(group_stack));
    let full_path = apply_group_prefix(group_stack, &route_path);

    extraction.interfaces.push(Interface {
        method: http_method,
        path: full_path.clone(),
        auth,
        anchor: anchor_from_node(node, file_path),
        parameters: common::extract_path_params(&full_path),
        handler_name: None,
        request_body_type: None,
    });
}

/// Expand `Route::resource()` or `Route::apiResource()` into individual routes.
///
/// Laravel resource routes follow a standard convention:
/// - `resource`: index, create, store, show, edit, update, destroy (7 routes)
/// - `apiResource`: index, store, show, update, destroy (5 routes — no create/edit)
fn expand_resource_routes(
    base_path: &str,
    api_only: bool,
    file_path: &Path,
    node: &Node,
    auth: Option<AuthKind>,
    extraction: &mut FileExtraction,
) {
    let base = base_path.trim_matches('/');
    // Infer singular form by stripping trailing 's' (simple heuristic)
    let singular = if base.ends_with('s') && base.len() > 1 {
        &base[..base.len() - 1]
    } else {
        base
    };
    let param = format!("{{{singular}}}");
    let anchor = anchor_from_node(node, file_path);

    // Standard resource routes
    let routes: &[(&str, &str)] = if api_only {
        &[
            ("get", ""),        // index
            ("post", ""),       // store
            ("get", &param),    // show
            ("put", &param),    // update
            ("delete", &param), // destroy
        ]
    } else {
        &[
            ("get", ""),        // index
            ("get", "create"),  // create
            ("post", ""),       // store
            ("get", &param),    // show
            ("get", "edit"),    // edit (simplified — actual is {param}/edit)
            ("put", &param),    // update
            ("delete", &param), // destroy
        ]
    };

    for (method_str, suffix) in routes {
        let method = common::parse_http_method(method_str).unwrap();
        let path = if suffix.is_empty() {
            format!("/{base}")
        } else {
            format!("/{base}/{suffix}")
        };

        extraction.interfaces.push(Interface {
            method,
            path: path.clone(),
            auth: auth.clone(),
            anchor: anchor.clone(),
            parameters: common::extract_path_params(&path),
            handler_name: None,
            request_body_type: None,
        });
    }
}

/// Detect `->middleware('auth')` in the parent chain.
///
/// Laravel routes can chain middleware:
/// ```php
/// Route::post('/orders', ...)->middleware('auth');
/// ```
fn detect_middleware_chain(node: &Node, source: &str) -> Option<AuthKind> {
    // Check if this node is the object of a member_call_expression
    // that calls ->middleware('auth')
    let parent = node.parent()?;

    if parent.kind() == "member_call_expression" {
        let full_text = node_text_ref(&parent, source);
        if full_text.contains("middleware") {
            // Extract the middleware name from the full text
            if let Some(mw_name) = extract_middleware_name(full_text)
                && patterns::is_auth_indicator(&mw_name) {
                    return Some(AuthKind::Middleware(mw_name));
                }
        }
    }

    None
}

/// Extract middleware name from text like `->middleware('auth')`.
fn extract_middleware_name(text: &str) -> Option<String> {
    let idx = text.find("middleware(")?;
    let rest = &text[idx + "middleware(".len()..];
    // Find the first quoted string
    for quote in ['\'', '"'] {
        if let Some(start) = rest.find(quote)
            && let Some(end) = rest[start + 1..].find(quote) {
                return Some(rest[start + 1..start + 1 + end].to_string());
            }
    }
    None
}

/// Try to extract an HTTP call from `Http::get(url)`.
fn try_extract_http_facade_call(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let scope = match node.child_by_field_name("scope") {
        Some(s) => s,
        None => return,
    };

    let scope_name = node_text_ref(&scope, source);
    if scope_name != "Http" && scope_name != "Guzzle" {
        return;
    }

    let method_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };

    let method_name = node_text_ref(&method_node, source);
    if !matches!(
        method_name,
        "get" | "post" | "put" | "patch" | "delete" | "head" | "request"
    ) {
        return;
    }

    let call_text = node_text(node, source);
    let display_text = truncate_call_text(call_text, 100);

    extraction.dependencies.push(Dependency {
        target: display_text,
        dependency_type: DependencyType::HttpCall,
        anchor: anchor_from_node(node, file_path),
    });
}

/// Find the first string literal argument in a PHP argument list.
fn find_first_string_arg(args_node: &Node, source: &str) -> Option<String> {
    for i in 0..args_node.named_child_count() {
        if let Some(child) = args_node.named_child(i as u32) {
            // PHP argument node might wrap the expression
            let text = if child.kind() == "argument" {
                // Get the inner expression
                child
                    .named_child(0)
                    .map(|inner| node_text(&inner, source))
                    .unwrap_or_else(|| node_text(&child, source))
            } else {
                node_text(&child, source)
            };

            if let Some(value) = extract_string_value(&text) {
                return Some(value);
            }
            // PHP also has encapsed_string (double-quoted with interpolation)
            // For simple cases, try stripping quotes directly
            let trimmed = text.trim();
            if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                || (trimmed.starts_with('"') && trimmed.ends_with('"'))
            {
                return Some(common::strip_quotes(trimmed));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    

    fn extract_php(source: &str) -> FileExtraction {
        let path = PathBuf::from("routes.php");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Php, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Php)
    }

    #[test]
    fn extracts_laravel_get_route() {
        let ext = extract_php(
            r#"<?php
Route::get('/users', [UserController::class, 'index']);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_laravel_post_route() {
        let ext = extract_php(
            r#"<?php
Route::post('/api/orders', [OrderController::class, 'store']);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn detects_middleware_auth() {
        let ext = extract_php(
            r#"<?php
Route::post('/api/orders', [OrderController::class, 'store'])->middleware('auth');
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn no_auth_when_no_middleware() {
        let ext = extract_php(
            r#"<?php
Route::get('/health', function () { return 'ok'; });
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn extracts_http_facade_call() {
        let ext = extract_php(
            r#"<?php
$response = Http::get('https://api.example.com/data');
?>"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_php(
            r#"<?php
Log::info("User email: " . $user->email);
?>"#,
        );
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }

    #[test]
    fn extracts_multiple_routes() {
        let ext = extract_php(
            r#"<?php
Route::get('/users', [UserController::class, 'index']);
Route::post('/users', [UserController::class, 'store']);
Route::delete('/users/{id}', [UserController::class, 'destroy']);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
    }

    // --- Resource routes ---

    #[test]
    fn extracts_laravel_resource_routes() {
        let ext = extract_php(
            r#"<?php
Route::resource('/photos', PhotoController::class);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 7, "resource() expands to 7 routes");
    }

    #[test]
    fn extracts_laravel_api_resource_routes() {
        let ext = extract_php(
            r#"<?php
Route::apiResource('/posts', PostController::class);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 5, "apiResource() expands to 5 routes");
    }

    #[test]
    fn extracts_laravel_any_route() {
        let ext = extract_php(
            r#"<?php
Route::any('/webhook', WebhookController::class);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::All);
        assert_eq!(ext.interfaces[0].path, "/webhook");
    }

    #[test]
    fn resource_routes_with_middleware() {
        let ext = extract_php(
            r#"<?php
Route::resource('/photos', PhotoController::class)->middleware('auth');
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 7, "resource() expands to 7 routes");
        assert!(
            ext.interfaces.iter().all(|i| i.auth.is_some()),
            "all resource routes inherit middleware auth"
        );
    }

    #[test]
    fn resource_route_paths_are_correct() {
        let ext = extract_php(
            r#"<?php
Route::resource('/photos', PhotoController::class);
?>"#,
        );
        let paths: Vec<&str> = ext.interfaces.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"/photos"), "index");
        assert!(paths.contains(&"/photos/create"), "create");
        assert!(paths.contains(&"/photos/{photo}"), "show (singular param)");
        assert!(paths.contains(&"/photos/edit"), "edit");
    }

    #[test]
    fn realistic_laravel_routes() {
        let ext = extract_php(
            r#"<?php
use Illuminate\Support\Facades\Route;

Route::get('/health', function () {
    return response()->json(['status' => 'ok']);
});

Route::post('/api/payments', [PaymentController::class, 'charge'])->middleware('auth');

Route::get('/api/products', [ProductController::class, 'index']);

$response = Http::post('https://stripe.api/charge', $data);
Log::info("Processing payment for: " . $request->email);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        assert!(ext.interfaces[0].auth.is_none()); // /health
        assert!(ext.interfaces[1].auth.is_some()); // /api/payments
        assert_eq!(ext.dependencies.len(), 1); // Http::post
        assert!(!ext.sinks.is_empty()); // Log::info
    }

    // --- Route group context propagation ---

    #[test]
    fn group_middleware_propagates_auth() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::middleware('auth:api')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 1);
        assert!(
            ext.interfaces[0].auth.is_some(),
            "route inside middleware group should inherit auth"
        );
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn group_prefix_prepends_path() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api/v1')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].path, "/api/v1/users",
            "route path should be prefixed with group prefix"
        );
    }

    #[test]
    fn group_chained_prefix_and_middleware() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api')->middleware('auth:api')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
    Route::post('/users', [UserController::class, 'store']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 2);
        for iface in &ext.interfaces {
            assert!(
                iface.path.starts_with("/api/"),
                "path '{}' should start with /api/",
                iface.path
            );
            assert!(
                iface.auth.is_some(),
                "route at '{}' should inherit auth from group",
                iface.path
            );
        }
        assert_eq!(ext.interfaces[0].path, "/api/users");
        assert_eq!(ext.interfaces[1].path, "/api/users");
    }

    #[test]
    fn group_nested() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api')->group(function () {
    Route::prefix('/v1')->group(function () {
        Route::get('/items', [ItemController::class, 'index']);
    });
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].path, "/api/v1/items",
            "nested groups should accumulate prefixes"
        );
    }

    #[test]
    fn group_does_not_leak() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api')->middleware('auth')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
});

Route::get('/health', function () { return 'ok'; });
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 2);

        let api_route = ext.interfaces.iter().find(|i| i.path == "/api/users");
        assert!(api_route.is_some(), "should find /api/users route");
        assert!(
            api_route.unwrap().auth.is_some(),
            "/api/users should have auth"
        );

        let health_route = ext.interfaces.iter().find(|i| i.path == "/health");
        assert!(health_route.is_some(), "should find /health route");
        assert!(
            health_route.unwrap().auth.is_none(),
            "/health should NOT have auth — group context must not leak"
        );
    }

    #[test]
    fn group_middleware_only_no_prefix() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::middleware('auth')->group(function () {
    Route::get('/dashboard', [DashboardController::class, 'index']);
    Route::post('/settings', [SettingsController::class, 'update']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 2);
        assert_eq!(
            ext.interfaces[0].path, "/dashboard",
            "path should be unchanged when group has no prefix"
        );
        assert_eq!(ext.interfaces[1].path, "/settings");
        assert!(
            ext.interfaces.iter().all(|i| i.auth.is_some()),
            "all routes should inherit auth from middleware-only group"
        );
    }
}
