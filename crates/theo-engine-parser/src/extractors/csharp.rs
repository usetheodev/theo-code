//! C# semantic extraction from tree-sitter CSTs.
//!
//! Handles the C# language family, focusing on ASP.NET Core:
//!
//! Extracts:
//! - `[HttpGet]`, `[HttpPost]`, etc. route attribute detection
//! - `[Authorize]` auth attribute detection
//! - `app.MapGroup()` prefix tracking for Minimal API route groups
//! - `HttpClient` HTTP calls (GetAsync, PostAsync, etc.)
//! - Log sinks with PII detection

use std::collections::HashMap;
use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::patterns;
use crate::tree_sitter::SupportedLanguage;
use crate::types::*;

use super::common::{self, anchor_from_node, node_text, node_text_ref, truncate_call_text};

/// Tracked info for a `MapGroup()` variable binding.
#[derive(Debug, Clone)]
struct GroupInfo {
    prefix: String,
    auth: Option<AuthKind>,
}

/// ASP.NET Core HTTP verb attributes → HTTP methods.
const HTTP_VERB_ATTRIBUTES: &[(&str, &str)] = &[
    ("HttpGet", "get"),
    ("HttpPost", "post"),
    ("HttpPut", "put"),
    ("HttpPatch", "patch"),
    ("HttpDelete", "delete"),
];

/// ASP.NET Minimal API verb methods → HTTP methods.
const MINIMAL_API_METHODS: &[(&str, &str)] = &[
    ("MapGet", "get"),
    ("MapPost", "post"),
    ("MapPut", "put"),
    ("MapPatch", "patch"),
    ("MapDelete", "delete"),
];

/// Extract semantic information from a C# source file.
pub fn extract(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
) -> FileExtraction {
    let root = tree.root_node();
    let mut extraction = common::new_extraction(file_path, language);
    let mut group_prefixes: HashMap<String, GroupInfo> = HashMap::new();

    extract_recursive(
        &root,
        source,
        file_path,
        &mut extraction,
        &mut group_prefixes,
    );

    extraction
}

fn extract_recursive(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
    group_prefixes: &mut HashMap<String, GroupInfo>,
) {
    match node.kind() {
        "class_declaration" => {
            try_extract_controller_routes(node, source, file_path, extraction);
        }
        "method_declaration"
            // Only extract standalone methods (not inside a class with [Route] prefix).
            // Methods inside a [Route] class are handled by try_extract_controller_routes.
            if !is_inside_route_class(node, source, file_path) => {
                try_extract_attributed_route(node, source, file_path, extraction, "", &None);
            }
        "local_declaration_statement" => {
            try_track_map_group(node, source, group_prefixes);
        }
        "expression_statement" => {
            try_track_map_group_assignment(node, source, group_prefixes);
        }
        "invocation_expression" => {
            try_extract_minimal_api_route(node, source, file_path, extraction, group_prefixes);
            try_extract_http_call(node, source, file_path, extraction);
            common::try_extract_log_sink(node, source, file_path, extraction);
        }
        _ => {}
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            extract_recursive(&child, source, file_path, extraction, group_prefixes);
        }
    }
}

// ---------------------------------------------------------------------------
// Controller-level route prefix composition
// ---------------------------------------------------------------------------

/// Check if a method_declaration is inside a class that has a [Route] attribute.
fn is_inside_route_class(method_node: &Node, source: &str, file_path: &Path) -> bool {
    let mut current = method_node.parent();
    while let Some(node) = current {
        if node.kind() == "class_declaration" {
            let attrs = collect_attributes(&node, source, file_path);
            return attrs.iter().any(|(name, _, _)| name == "Route");
        }
        current = node.parent();
    }
    false
}

/// Extract routes from an ASP.NET controller class with `[Route]` prefix.
///
/// Handles the common pattern:
/// ```csharp
/// [Route("api/[controller]")]
/// [Authorize]
/// public class ProductsController : ControllerBase {
///     [HttpGet("{id}")]
///     public IActionResult Get(int id) { ... }
/// }
/// ```
fn try_extract_controller_routes(
    class_node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let class_attrs = collect_attributes(class_node, source, file_path);

    // Find [Route("prefix")] on the class
    let raw_prefix = class_attrs
        .iter()
        .find(|(name, _, _)| name == "Route")
        .and_then(|(_, text, _)| extract_path_from_attribute_text(text));

    // No [Route] attribute — methods are handled by the standalone method_declaration path
    let raw_prefix = match raw_prefix {
        Some(p) => p,
        None => return,
    };

    // Extract class name for [controller] token replacement
    let class_name = class_node
        .child_by_field_name("name")
        .map(|n| node_text(&n, source))
        .unwrap_or_default();

    let controller_name = class_name
        .strip_suffix("Controller")
        .unwrap_or(&class_name)
        .to_lowercase();

    let prefix = raw_prefix.replace("[controller]", &controller_name);

    // Class-level [Authorize]
    let class_auth = class_attrs
        .iter()
        .find(|(name, _, _)| is_auth_attribute(name))
        .map(|(name, _, _)| AuthKind::Attribute(name.clone()));

    // Walk class body for method declarations
    let body = match find_child_by_kind(class_node, "declaration_list") {
        Some(b) => b,
        None => return,
    };

    for i in 0..body.child_count() {
        if let Some(child) = body.child(i as u32)
            && child.kind() == "method_declaration" {
                try_extract_attributed_route(
                    &child,
                    source,
                    file_path,
                    extraction,
                    &prefix,
                    &class_auth,
                );
            }
    }
}

/// Try to extract a route from a method with HTTP verb attributes.
///
/// Handles ASP.NET Core patterns:
/// ```csharp
/// [HttpGet("users")]
/// public IActionResult GetUsers() { ... }
///
/// [HttpPost("api/orders")]
/// [Authorize(Roles = "Admin")]
/// public IActionResult CreateOrder([FromBody] OrderDto dto) { ... }
/// ```
///
/// When `class_prefix` is non-empty, the final path is composed as
/// `class_prefix + "/" + method_path`.
fn try_extract_attributed_route(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
    class_prefix: &str,
    class_auth: &Option<AuthKind>,
) {
    let attributes = collect_attributes(node, source, file_path);

    let mut route_info: Option<(HttpMethod, String, SourceAnchor)> = None;
    let mut method_auth: Option<AuthKind> = None;
    let mut has_allow_anonymous = false;

    for (attr_name, attr_text, attr_anchor) in &attributes {
        // Check for HTTP verb attributes
        if let Some((method, path)) = try_parse_http_attribute(attr_name, attr_text) {
            route_info = Some((method, path, attr_anchor.clone()));
        }

        // Check for Route attribute (class-level prefix, also sometimes on methods)
        if *attr_name == "Route" && route_info.is_none() {
            let path = extract_path_from_attribute_text(attr_text).unwrap_or_default();
            route_info = Some((HttpMethod::All, path, attr_anchor.clone()));
        }

        // Check for auth attributes
        if *attr_name == "AllowAnonymous" {
            has_allow_anonymous = true;
        } else if method_auth.is_none() && is_auth_attribute(attr_name) {
            method_auth = Some(AuthKind::Attribute(attr_name.clone()));
        }
    }

    if let Some((method, method_path, anchor)) = route_info {
        // Compose path with class prefix
        let path = compose_csharp_path(class_prefix, &method_path);

        // Auth resolution: AllowAnonymous > method auth > class auth
        let auth = if has_allow_anonymous {
            None
        } else {
            method_auth.or_else(|| class_auth.clone())
        };

        // Extract method name from the method_declaration node
        let handler_name = node
            .child_by_field_name("name")
            .map(|n| node_text(&n, source));

        extraction.interfaces.push(Interface {
            method,
            path: path.clone(),
            auth,
            anchor,
            parameters: common::extract_path_params(&path),
            handler_name,
            request_body_type: None,
        });
    }
}

/// Compose a full path from class-level prefix and method-level path.
///
/// `[Route("api/v1/products")]` + `[HttpGet("")]` → `/api/v1/products`
/// `[Route("api/[controller]")]` + `[HttpGet("{id}")]` → `/api/products/{id}`
fn compose_csharp_path(prefix: &str, method_path: &str) -> String {
    let prefix = prefix.trim_matches('/');
    let method_path = method_path.trim_matches('/');

    if prefix.is_empty() && method_path.is_empty() {
        "/".to_string()
    } else if prefix.is_empty() {
        format!("/{method_path}")
    } else if method_path.is_empty() {
        format!("/{prefix}")
    } else {
        format!("/{prefix}/{method_path}")
    }
}

/// Collect attributes from a C# method declaration.
///
/// In C#'s tree-sitter grammar, attributes appear in `attribute_list`
/// children of the method declaration, containing one or more `attribute` nodes.
fn collect_attributes(
    node: &Node,
    source: &str,
    file_path: &Path,
) -> Vec<(String, String, SourceAnchor)> {
    let mut result = Vec::new();

    // Check direct children for attribute_list
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "attribute_list" {
                for j in 0..child.child_count() {
                    if let Some(attr) = child.child(j as u32)
                        && attr.kind() == "attribute"
                            && let Some(name) = extract_attribute_name(&attr, source) {
                                let text = node_text(&attr, source);
                                result.push((name, text, anchor_from_node(&attr, file_path)));
                            }
                }
            }
    }

    // Also check preceding siblings — attribute_list can be siblings
    let mut prev = node.prev_named_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "attribute_list" {
            for j in 0..sibling.child_count() {
                if let Some(attr) = sibling.child(j as u32)
                    && attr.kind() == "attribute"
                        && let Some(name) = extract_attribute_name(&attr, source) {
                            let text = node_text(&attr, source);
                            result.push((name, text, anchor_from_node(&attr, file_path)));
                        }
            }
        } else {
            break;
        }
        prev = sibling.prev_named_sibling();
    }

    result
}

/// Extract the attribute name from an `attribute` node.
fn extract_attribute_name(node: &Node, source: &str) -> Option<String> {
    // C# attribute node has a `name` field
    if let Some(name_node) = node.child_by_field_name("name") {
        let text = node_text(&name_node, source);
        // Remove namespace prefix if present (e.g., "Microsoft.AspNetCore.Authorization.Authorize" → "Authorize")
        return Some(text.rsplit('.').next().unwrap_or(&text).to_string());
    }

    // Fallback: find the first identifier child
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && (child.kind() == "identifier" || child.kind() == "generic_name") {
                return Some(node_text(&child, source));
            }
    }

    // Text-based fallback
    let text = node_text(node, source);
    let name = text.split('(').next()?.trim().trim_start_matches('[');
    if !name.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

/// Parse an HTTP verb attribute into method + path.
fn try_parse_http_attribute(attr_name: &str, attr_text: &str) -> Option<(HttpMethod, String)> {
    for (verb, method_str) in HTTP_VERB_ATTRIBUTES {
        if attr_name == *verb {
            let method = common::parse_http_method(method_str)?;
            let path = extract_path_from_attribute_text(attr_text).unwrap_or_default();
            return Some((method, path));
        }
    }
    None
}

/// Extract a path string from attribute text like `HttpGet("users")`.
fn extract_path_from_attribute_text(attr_text: &str) -> Option<String> {
    // Find quoted string in attribute
    for quote in ['"', '\''] {
        if let Some(start) = attr_text.find(quote)
            && let Some(end) = attr_text[start + 1..].find(quote) {
                let path = &attr_text[start + 1..start + 1 + end];
                // Normalize: add leading / if missing
                return if path.starts_with('/') {
                    Some(path.to_string())
                } else {
                    Some(format!("/{path}"))
                };
            }
    }
    None
}

/// Check if an attribute name indicates auth.
fn is_auth_attribute(name: &str) -> bool {
    // AllowAnonymous explicitly removes auth — don't count it
    if name == "AllowAnonymous" {
        return false;
    }
    name == "Authorize" || patterns::is_auth_indicator(name)
}

// ---------------------------------------------------------------------------
// Minimal API extraction
// ---------------------------------------------------------------------------

/// Try to extract a Minimal API route from `app.MapGet("/items", handler)`.
///
/// Handles ASP.NET Minimal API patterns (.NET 6+):
/// ```csharp
/// app.MapGet("/items", () => Results.Ok());
/// app.MapPost("/items", handler).RequireAuthorization();
/// var api = app.MapGroup("/api"); api.MapGet("/items", handler);
/// ```
fn try_extract_minimal_api_route(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
    group_prefixes: &HashMap<String, GroupInfo>,
) {
    // Get the method name from the invocation
    let method_name = match extract_invocation_method_name(node, source) {
        Some(name) => name,
        None => return,
    };

    // Match against Minimal API verbs
    let http_method = match MINIMAL_API_METHODS
        .iter()
        .find(|(verb, _)| *verb == method_name.as_str())
    {
        Some((_, method_str)) => match common::parse_http_method(method_str) {
            Some(m) => m,
            None => return,
        },
        None => return,
    };

    // Extract path from first argument
    let raw_path = match extract_first_string_argument(node, source) {
        Some(p) => p,
        None => return,
    };

    // Resolve group prefix from receiver variable (e.g., `api` in `api.MapGet(...)`)
    let receiver_group =
        extract_invocation_receiver(node, source).and_then(|recv| group_prefixes.get(&recv));

    let path = match receiver_group {
        Some(group) => compose_csharp_path(&group.prefix, raw_path.trim_start_matches('/')),
        None => raw_path,
    };

    // Detect .RequireAuthorization() chaining on the route itself
    let route_auth = detect_minimal_api_auth(node, source);

    // Auth resolution: route-level auth > group-level auth
    let auth = route_auth.or_else(|| receiver_group.and_then(|g| g.auth.clone()));

    extraction.interfaces.push(Interface {
        method: http_method,
        path: path.clone(),
        auth,
        anchor: anchor_from_node(node, file_path),
        parameters: common::extract_path_params(&path),
        handler_name: None,
        request_body_type: None,
    });
}

/// Extract the method name from an invocation_expression.
///
/// For `app.MapGet(...)`, the structure is:
/// invocation_expression → member_access_expression → name: "MapGet"
fn extract_invocation_method_name(node: &Node, source: &str) -> Option<String> {
    // The function child is a member_access_expression
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "member_access_expression" {
                // The method name is the `name` field
                if let Some(name_node) = child.child_by_field_name("name") {
                    return Some(node_text(&name_node, source));
                }
            }
    }
    None
}

/// Extract the receiver (object) name from an `invocation_expression`.
///
/// For `api.MapGet(...)`, the structure is:
/// invocation_expression → member_access_expression → expression: "api"
///
/// Returns the identifier text of the receiver, if it is a simple identifier.
fn extract_invocation_receiver(node: &Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "member_access_expression"
                && let Some(expr) = child.child_by_field_name("expression")
                    && expr.kind() == "identifier" {
                        return Some(node_text(&expr, source));
                    }
    }
    None
}

/// Extract the first string literal argument from an invocation_expression.
fn extract_first_string_argument(node: &Node, source: &str) -> Option<String> {
    let args = find_child_by_kind(node, "argument_list")?;
    for i in 0..args.named_child_count() {
        if let Some(child) = args.named_child(i as u32) {
            let text = if child.kind() == "argument" {
                child
                    .named_child(0)
                    .map(|inner| node_text(&inner, source))
                    .unwrap_or_else(|| node_text(&child, source))
            } else {
                node_text(&child, source)
            };

            if let Some(value) = common::extract_string_value(&text) {
                return Some(value);
            }
        }
    }
    None
}

/// Detect `.RequireAuthorization()` in the parent chain.
///
/// When chained: `app.MapGet(...).RequireAuthorization()`, the CST nests as:
/// invocation_expression (RequireAuthorization)
///   └─ member_access_expression
///       └─ invocation_expression (MapGet)
///
/// We check if any parent invocation calls RequireAuthorization.
fn detect_minimal_api_auth(node: &Node, source: &str) -> Option<AuthKind> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            "member_access_expression" => {
                // Check the method name
                if let Some(name_node) = current.child_by_field_name("name") {
                    let name = node_text_ref(&name_node, source);
                    if name == "RequireAuthorization" {
                        return Some(AuthKind::Attribute("RequireAuthorization".into()));
                    }
                    if name == "AllowAnonymous" {
                        return None;
                    }
                }
            }
            "invocation_expression" => {
                // Check this invocation's method
                if let Some(method_name) = extract_invocation_method_name(&current, source) {
                    if method_name == "RequireAuthorization" {
                        return Some(AuthKind::Attribute("RequireAuthorization".into()));
                    }
                    if method_name == "AllowAnonymous" {
                        return None;
                    }
                }
            }
            // Stop walking at statement boundaries
            "expression_statement" | "local_declaration_statement" | "block" => break,
            _ => {}
        }
        current = current.parent()?;
    }
    None
}

// ---------------------------------------------------------------------------
// MapGroup prefix tracking
// ---------------------------------------------------------------------------

/// Track `var api = app.MapGroup("/api");` declarations.
///
/// Detects `local_declaration_statement` containing a `variable_declarator`
/// whose initializer calls `MapGroup`. Handles chained `.RequireAuthorization()`.
fn try_track_map_group(node: &Node, source: &str, group_prefixes: &mut HashMap<String, GroupInfo>) {
    // local_declaration_statement → variable_declaration → variable_declarator
    let var_decl = match find_descendant_by_kind(node, "variable_declaration") {
        Some(d) => d,
        None => return,
    };

    for i in 0..var_decl.child_count() {
        if let Some(declarator) = var_decl.child(i as u32) {
            if declarator.kind() != "variable_declarator" {
                continue;
            }

            let var_name = match declarator.child_by_field_name("name") {
                Some(n) if n.kind() == "identifier" => node_text(&n, source),
                _ => continue,
            };

            // The initializer may be in an equals_value_clause or directly
            // as a child of the variable_declarator (C# tree-sitter grammar).
            // Try equals_value_clause first, then fall back to direct children.
            if let Some(eq) = find_child_by_kind(&declarator, "equals_value_clause")
                && let Some(info) = try_extract_map_group_from_expr(&eq, source, group_prefixes) {
                    group_prefixes.insert(var_name.clone(), info);
                    continue;
                }

            // Direct child: variable_declarator → invocation_expression
            if let Some(info) = try_extract_map_group_from_expr(&declarator, source, group_prefixes)
            {
                group_prefixes.insert(var_name, info);
            }
        }
    }
}

/// Track `api = app.MapGroup("/api");` assignment expressions.
fn try_track_map_group_assignment(
    node: &Node,
    source: &str,
    group_prefixes: &mut HashMap<String, GroupInfo>,
) {
    // expression_statement → assignment_expression
    let assignment = match find_child_by_kind(node, "assignment_expression") {
        Some(a) => a,
        None => return,
    };

    let var_name = match assignment.child_by_field_name("left") {
        Some(n) if n.kind() == "identifier" => node_text(&n, source),
        _ => return,
    };

    let right = match assignment.child_by_field_name("right") {
        Some(r) => r,
        None => return,
    };

    if let Some(info) = try_extract_map_group_from_node(&right, source, group_prefixes) {
        group_prefixes.insert(var_name, info);
    }
}

/// Try to extract `MapGroup` info from an expression (within equals_value_clause or assignment RHS).
///
/// Walks children to find the invocation_expression containing the `MapGroup` call.
fn try_extract_map_group_from_expr(
    node: &Node,
    source: &str,
    group_prefixes: &HashMap<String, GroupInfo>,
) -> Option<GroupInfo> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && let Some(info) = try_extract_map_group_from_node(&child, source, group_prefixes) {
                return Some(info);
            }
    }
    None
}

/// Try to extract `MapGroup` info from a node that may be an invocation_expression.
///
/// Handles:
/// - `app.MapGroup("/api")` — direct call
/// - `app.MapGroup("/api").RequireAuthorization()` — outer invocation wrapping inner MapGroup
fn try_extract_map_group_from_node(
    node: &Node,
    source: &str,
    group_prefixes: &HashMap<String, GroupInfo>,
) -> Option<GroupInfo> {
    if node.kind() != "invocation_expression" {
        return None;
    }

    let method_name = extract_invocation_method_name(node, source)?;

    if method_name == "MapGroup" {
        let prefix_arg = extract_first_string_argument(node, source)?;

        // Check if the receiver is itself a tracked group variable (nested groups)
        let receiver_prefix = extract_invocation_receiver(node, source)
            .and_then(|recv| group_prefixes.get(&recv))
            .map(|g| g.prefix.as_str())
            .unwrap_or("");

        let full_prefix = compose_csharp_path(receiver_prefix, prefix_arg.trim_start_matches('/'));
        return Some(GroupInfo {
            prefix: full_prefix,
            auth: None,
        });
    }

    // Check for chaining: `something.RequireAuthorization()` where `something` is a MapGroup call
    if method_name == "RequireAuthorization" {
        // The receiver of RequireAuthorization is the inner MapGroup invocation
        // CST: invocation_expression(RequireAuthorization) → member_access_expression
        //        → invocation_expression(MapGroup)
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && child.kind() == "member_access_expression"
                    && let Some(expr) = child.child_by_field_name("expression")
                        && let Some(mut info) =
                            try_extract_map_group_from_node(&expr, source, group_prefixes)
                        {
                            info.auth = Some(AuthKind::Attribute("RequireAuthorization".into()));
                            return Some(info);
                        }
        }
    }

    None
}

/// Find a descendant node by kind (up to 2 levels deep).
fn find_descendant_by_kind<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == kind {
                return Some(child);
            }
    }
    // One level deeper
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            for j in 0..child.child_count() {
                if let Some(grandchild) = child.child(j as u32)
                    && grandchild.kind() == kind {
                        return Some(grandchild);
                    }
            }
        }
    }
    None
}

/// Find a direct child node by its kind.
fn find_child_by_kind<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == kind {
                return Some(child);
            }
    }
    None
}

/// Try to extract an HTTP call from `HttpClient.GetAsync(...)` etc.
fn try_extract_http_call(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let call_ref = node_text_ref(node, source);
    let call_lower = call_ref.to_lowercase();

    let is_http_client = (call_lower.contains("httpclient") || call_lower.contains("client"))
        && (call_lower.contains("getasync")
            || call_lower.contains("postasync")
            || call_lower.contains("putasync")
            || call_lower.contains("deleteasync")
            || call_lower.contains("sendasync")
            || call_lower.contains("getstringasync"));

    if !is_http_client {
        return;
    }

    let display_text = truncate_call_text(call_ref.to_string(), 100);

    extraction.dependencies.push(Dependency {
        target: display_text,
        dependency_type: DependencyType::HttpCall,
        anchor: anchor_from_node(node, file_path),
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    

    fn extract_cs(source: &str) -> FileExtraction {
        let path = PathBuf::from("Controller.cs");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::CSharp, None)
                .unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::CSharp)
    }

    #[test]
    fn extracts_http_get_route() {
        let ext = extract_cs(
            r#"
public class UsersController : ControllerBase {
    [HttpGet("users")]
    public IActionResult GetUsers() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_http_post_with_path() {
        let ext = extract_cs(
            r#"
public class OrdersController : ControllerBase {
    [HttpPost("api/orders")]
    public IActionResult CreateOrder([FromBody] OrderDto dto) {
        return Created();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn detects_authorize_attribute() {
        let ext = extract_cs(
            r#"
public class SecureController : ControllerBase {
    [HttpGet("api/secure")]
    [Authorize]
    public IActionResult GetSecure() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Attribute("Authorize".into()))
        );
    }

    #[test]
    fn detects_authorize_with_roles() {
        let ext = extract_cs(
            r#"
public class AdminController : ControllerBase {
    [HttpDelete("api/items/{id}")]
    [Authorize(Roles = "Admin")]
    public IActionResult DeleteItem(int id) {
        return NoContent();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn no_auth_on_allow_anonymous() {
        let ext = extract_cs(
            r#"
public class PublicController : ControllerBase {
    [HttpGet("health")]
    [AllowAnonymous]
    public IActionResult Health() {
        return Ok("healthy");
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn extracts_http_client_call() {
        let ext = extract_cs(
            r#"
public class PaymentService {
    public async Task<string> Charge() {
        var client = new HttpClient();
        var response = await client.GetAsync("https://payment.api/charge");
        return await response.Content.ReadAsStringAsync();
    }
}
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_cs(
            r#"
public class Handler {
    public void Handle() {
        Logger.info("User email: " + user.email);
    }
}
"#,
        );
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }

    // --- Class-level [Route] prefix ---

    #[test]
    fn composes_class_route_prefix_with_method() {
        let ext = extract_cs(
            r#"
[Route("api/v1/products")]
public class ProductsController : ControllerBase {
    [HttpGet("")]
    public IActionResult List() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].path, "/api/v1/products");
    }

    #[test]
    fn replaces_controller_token() {
        let ext = extract_cs(
            r#"
[Route("api/[controller]")]
public class ProductsController : ControllerBase {
    [HttpGet("{id}")]
    public IActionResult Get(int id) {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].path, "/api/products/{id}");
    }

    #[test]
    fn class_authorize_applies_to_methods() {
        let ext = extract_cs(
            r#"
[Route("api/[controller]")]
[Authorize]
public class SecureController : ControllerBase {
    [HttpGet("")]
    public IActionResult List() {
        return Ok();
    }

    [HttpPost("")]
    public IActionResult Create() {
        return Ok();
    }

    [HttpGet("public")]
    [AllowAnonymous]
    public IActionResult Public() {
        return Ok();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        // List and Create inherit class [Authorize]
        assert!(ext.interfaces[0].auth.is_some(), "List has auth");
        assert!(ext.interfaces[1].auth.is_some(), "Create has auth");
        // Public has [AllowAnonymous] which nullifies class auth
        assert!(
            ext.interfaces[2].auth.is_none(),
            "Public has no auth (AllowAnonymous)"
        );
    }

    // --- Minimal API ---

    #[test]
    fn extracts_minimal_api_get_route() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
app.MapGet("/items", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/items");
    }

    #[test]
    fn extracts_minimal_api_post_route() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
app.MapPost("/items", (ItemDto item) => Results.Created());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/items");
    }

    #[test]
    fn detects_minimal_api_require_authorization() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
app.MapGet("/secret", () => Results.Ok()).RequireAuthorization();
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Attribute("RequireAuthorization".into()))
        );
    }

    #[test]
    fn realistic_minimal_api_program() {
        let ext = extract_cs(
            r#"
var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

app.MapGet("/health", () => Results.Ok("healthy"));

app.MapGet("/api/items", () => Results.Ok(new List<Item>()));

app.MapPost("/api/items", (ItemDto item) => {
    Logger.info("Creating item for: " + item.email);
    return Results.Created();
}).RequireAuthorization();

app.MapDelete("/api/items/{id}", (int id) => Results.NoContent())
    .RequireAuthorization();

app.Run();
"#,
        );
        assert_eq!(ext.interfaces.len(), 4, "4 Minimal API routes");
        assert!(ext.interfaces[0].auth.is_none(), "/health has no auth");
        assert!(
            ext.interfaces[1].auth.is_none(),
            "GET /api/items has no auth"
        );
        assert!(ext.interfaces[2].auth.is_some(), "POST /api/items has auth");
        assert!(ext.interfaces[3].auth.is_some(), "DELETE has auth");
        assert!(
            ext.sinks.iter().any(|s| s.contains_pii),
            "PII in log detected"
        );
    }

    #[test]
    fn realistic_aspnet_controller() {
        let ext = extract_cs(
            r#"
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;

[ApiController]
[Route("api/v1/products")]
public class ProductsController : ControllerBase {

    [HttpGet("")]
    public IActionResult List() {
        Logger.info("Listing products");
        return Ok();
    }

    [HttpPost("")]
    [Authorize(Roles = "Admin")]
    public IActionResult Create([FromBody] ProductDto dto) {
        Logger.info("Creating product for: " + dto.email);
        return Created();
    }

    [HttpDelete("{id}")]
    [Authorize]
    public async Task<IActionResult> Delete(int id) {
        var client = new HttpClient();
        await client.PostAsync("https://audit.service/log", null);
        return NoContent();
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        let authed: Vec<_> = ext.interfaces.iter().filter(|i| i.auth.is_some()).collect();
        assert_eq!(authed.len(), 2);
        assert_eq!(ext.dependencies.len(), 1);
        assert!(!ext.sinks.is_empty());
    }

    // --- MapGroup prefix tracking ---

    #[test]
    fn mapgroup_prefix_basic() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
var api = app.MapGroup("/api");
api.MapGet("/items", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/items");
    }

    #[test]
    fn mapgroup_prefix_nested() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
var api = app.MapGroup("/api");
var v1 = api.MapGroup("/v1");
v1.MapGet("/items", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/v1/items");
    }

    #[test]
    fn mapgroup_with_auth() {
        let ext = extract_cs(
            r#"
var app = builder.Build();
var admin = app.MapGroup("/admin").RequireAuthorization();
admin.MapGet("/users", () => Results.Ok());
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].path, "/admin/users");
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Attribute("RequireAuthorization".into())),
            "Route inherits auth from MapGroup"
        );
    }
}
