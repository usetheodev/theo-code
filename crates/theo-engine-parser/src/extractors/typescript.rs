//! TypeScript/JavaScript semantic extraction from tree-sitter CSTs.
//!
//! Handles the JavaScript-like language family (TS, TSX, JS, JSX).
//! These languages share identical CST node names for the constructs
//! we extract: call_expression, member_expression, import_statement.
//!
//! Extracts:
//! - Express/Koa/Hapi route definitions (app.get, router.post, etc.)
//! - Auth middleware detection
//! - External HTTP calls (fetch, axios)
//! - Log sinks with PII detection
//! - Import/require statements

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::patterns;
use crate::tree_sitter::SupportedLanguage;
use crate::types::*;

use super::common::{
    self, ArgumentInfo, anchor_from_node, collect_arguments, extract_string_value, node_text,
    node_text_ref, strip_quotes, truncate_call_text,
};

/// NestJS HTTP route decorator names → HTTP methods.
const NESTJS_ROUTE_DECORATORS: &[(&str, &str)] = &[
    ("Get", "get"),
    ("Post", "post"),
    ("Put", "put"),
    ("Patch", "patch"),
    ("Delete", "delete"),
    ("Options", "options"),
    ("Head", "head"),
    ("All", "all"),
];

/// Express-style HTTP route methods (lowercase for JS/TS).
const ROUTE_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "all",
];

/// Extract semantic information from a TypeScript/JavaScript source file.
pub fn extract(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
) -> FileExtraction {
    let root = tree.root_node();
    let mut extraction = common::new_extraction(file_path, language);

    extract_recursive(&root, source, file_path, &mut extraction);

    extraction
}

fn extract_recursive(node: &Node, source: &str, file_path: &Path, extraction: &mut FileExtraction) {
    match node.kind() {
        "call_expression" => {
            try_extract_route(node, source, file_path, extraction);
            try_extract_http_call(node, source, file_path, extraction);
            try_extract_log_sink(node, source, file_path, extraction);
        }
        "class_declaration" => {
            try_extract_nestjs_controller(node, source, file_path, extraction);
        }
        "import_statement" => {
            try_extract_import(node, source, extraction);
        }
        _ => {}
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            extract_recursive(&child, source, file_path, extraction);
        }
    }
}

/// Try to extract an Express route from `app.get('/path', handler)`.
fn try_extract_route(node: &Node, source: &str, file_path: &Path, extraction: &mut FileExtraction) {
    let function_node = match node.child_by_field_name("function") {
        Some(n) if n.kind() == "member_expression" => n,
        _ => return,
    };

    let method_node = match function_node.child_by_field_name("property") {
        Some(n) => n,
        None => return,
    };

    let method_name = node_text_ref(&method_node, source);
    if !ROUTE_METHODS.contains(&method_name) {
        return;
    }

    let http_method = match common::parse_http_method(method_name) {
        Some(m) => m,
        None => return,
    };

    let args_node = match node.child_by_field_name("arguments") {
        Some(n) => n,
        None => return,
    };

    let args = collect_arguments(&args_node, source);
    if args.is_empty() {
        return;
    }

    let route_path = match extract_string_value(&args[0].text) {
        Some(p) => p,
        None => return,
    };

    let auth = detect_auth_middleware(&args[1..]);

    // The last argument is typically the handler function.
    // If it's an identifier (named function reference), capture its name.
    let handler_name = args.last().and_then(|arg| {
        let trimmed = arg.text.trim();
        // Only capture simple identifiers, not arrow functions or other expressions
        if !trimmed.is_empty()
            && !trimmed.contains('(')
            && !trimmed.contains(')')
            && !trimmed.contains('=')
            && !trimmed.contains('{')
            && trimmed.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            Some(trimmed.to_string())
        } else {
            None
        }
    });

    extraction.interfaces.push(Interface {
        method: http_method,
        path: route_path.clone(),
        auth,
        anchor: anchor_from_node(node, file_path),
        parameters: common::extract_path_params(&route_path),
        handler_name,
        request_body_type: None,
    });
}

fn detect_auth_middleware(args: &[ArgumentInfo]) -> Option<AuthKind> {
    if args.len() < 2 {
        return None;
    }

    let middlewares = &args[..args.len() - 1];
    for mw in middlewares {
        if patterns::is_auth_indicator(&mw.text) {
            return Some(AuthKind::Middleware(mw.text.clone()));
        }
    }

    None
}

/// Try to extract an HTTP call from `fetch(url)` or `axios.get(url)`.
fn try_extract_http_call(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let function_node = match node.child_by_field_name("function") {
        Some(n) => n,
        None => return,
    };

    let is_fetch =
        function_node.kind() == "identifier" && node_text_ref(&function_node, source) == "fetch";

    let is_axios = function_node.kind() == "member_expression"
        && function_node
            .child_by_field_name("object")
            .map(|obj| node_text_ref(&obj, source) == "axios")
            .unwrap_or(false);

    if !is_fetch && !is_axios {
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

/// Try to extract a log sink from `console.log(...)`, `logger.info(...)`, etc.
fn try_extract_log_sink(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let function_node = match node.child_by_field_name("function") {
        Some(n) if n.kind() == "member_expression" => n,
        _ => return,
    };

    let object_name = match function_node.child_by_field_name("object") {
        Some(obj) => node_text_ref(&obj, source),
        None => return,
    };

    let method_name = match function_node.child_by_field_name("property") {
        Some(prop) => node_text_ref(&prop, source),
        None => return,
    };

    if !patterns::LOG_OBJECTS.contains(&object_name) {
        return;
    }
    if !patterns::LOG_METHODS.contains(&method_name) {
        return;
    }

    let call_text = node_text(node, source);
    let pii = patterns::contains_pii(&call_text);

    extraction.sinks.push(Sink {
        sink_type: SinkType::Log,
        anchor: anchor_from_node(node, file_path),
        text: call_text,
        contains_pii: pii,
    });
}

fn try_extract_import(node: &Node, source: &str, extraction: &mut FileExtraction) {
    let source_node = node.child_by_field_name("source");
    let import_source = match source_node {
        Some(n) => strip_quotes(&node_text(&n, source)),
        None => return,
    };

    let mut specifiers = Vec::new();
    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i as u32) {
            collect_import_specifiers(&child, source, &mut specifiers);
        }
    }

    extraction.imports.push(ImportInfo {
        source: import_source,
        specifiers,
        line: node.start_position().row + 1,
        aliases: vec![],
    });
}

// ---------------------------------------------------------------------------
// NestJS decorator-based routing
// ---------------------------------------------------------------------------

/// Extract routes from a NestJS `@Controller()` class.
///
/// NestJS uses decorator-based routing:
/// ```typescript
/// @Controller('articles')
/// @UseGuards(AuthGuard('jwt'))
/// export class ArticlesController {
///     @Get(':slug')
///     findOne(@Param('slug') slug: string) { ... }
///
///     @Post()
///     @UseGuards(AuthGuard('jwt'))
///     create(@Body() dto: CreateDto) { ... }
/// }
/// ```
fn try_extract_nestjs_controller(
    class_node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let decorators = collect_decorators(class_node, source);

    // Find @Controller('prefix') decorator
    let controller_prefix = decorators
        .iter()
        .find(|(name, _, _)| name == "Controller")
        .and_then(|(_, arg, _)| arg.clone())
        .unwrap_or_default();

    // Find class-level @UseGuards(...) for auth
    let class_auth = detect_nestjs_auth(&decorators);

    // Find the class body
    let body = match find_child_by_kind(class_node, "class_body") {
        Some(b) => b,
        None => return,
    };

    // Walk class body for method definitions
    for i in 0..body.child_count() {
        if let Some(child) = body.child(i as u32) {
            if child.kind() == "method_definition" {
                try_extract_nestjs_method(
                    &child,
                    source,
                    file_path,
                    &controller_prefix,
                    &class_auth,
                    extraction,
                );
            }
        }
    }
}

/// Extract a single NestJS method with its route decorators.
fn try_extract_nestjs_method(
    method_node: &Node,
    source: &str,
    file_path: &Path,
    controller_prefix: &str,
    class_auth: &Option<AuthKind>,
    extraction: &mut FileExtraction,
) {
    let decorators = collect_decorators(method_node, source);

    // Find the HTTP method decorator
    let route_info = decorators.iter().find_map(|(name, arg, _)| {
        for (dec_name, method_str) in NESTJS_ROUTE_DECORATORS {
            if name == *dec_name {
                let method = common::parse_http_method(method_str)?;
                return Some((method, arg.clone()));
            }
        }
        None
    });

    let (http_method, subpath) = match route_info {
        Some((m, s)) => (m, s),
        None => return,
    };

    let path = compose_nestjs_path(controller_prefix, subpath.as_deref());

    // Method-level auth overrides class-level
    let method_auth = detect_nestjs_auth(&decorators);
    let auth = method_auth.or_else(|| class_auth.clone());

    // Extract the method name from the method_definition node
    let handler_name = method_node
        .child_by_field_name("name")
        .map(|name_node| node_text_ref(&name_node, source).to_string());

    extraction.interfaces.push(Interface {
        method: http_method,
        path: path.clone(),
        auth,
        anchor: anchor_from_node(method_node, file_path),
        parameters: common::extract_path_params(&path),
        handler_name,
        request_body_type: None,
    });
}

/// Collect decorators from a class or method node.
///
/// Returns `(decorator_name, first_string_arg_or_None, line)` tuples.
///
/// In tree-sitter-typescript:
/// - Class decorators are **children** of `class_declaration`
/// - Method decorators are **preceding siblings** of `method_definition` within `class_body`
fn collect_decorators(node: &Node, source: &str) -> Vec<(String, Option<String>, usize)> {
    let mut result = Vec::new();

    // Strategy 1: Check direct children (works for class_declaration)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "decorator" {
                if let Some(entry) = parse_decorator_node(&child, source) {
                    result.push(entry);
                }
            }
        }
    }

    // Strategy 2: Check preceding siblings (works for method_definition)
    if result.is_empty() {
        let mut prev = node.prev_named_sibling();
        while let Some(sib) = prev {
            if sib.kind() == "decorator" {
                if let Some(entry) = parse_decorator_node(&sib, source) {
                    result.push(entry);
                }
                prev = sib.prev_named_sibling();
            } else {
                break;
            }
        }
        // Reverse to maintain source order (we collected bottom-up)
        result.reverse();
    }

    result
}

/// Parse a single `decorator` CST node into (name, first_string_arg, line).
///
/// For decorators like `@UseGuards(AuthGuard('jwt'))`, the first argument
/// is NOT a string literal — it's a call expression. We capture:
/// - `first_string_arg`: the first simple string argument (e.g., 'articles')
/// - `first_arg_text`: the raw text of the first argument for complex expressions
fn parse_decorator_node(decorator: &Node, source: &str) -> Option<(String, Option<String>, usize)> {
    let line = decorator.start_position().row + 1;

    // The decorator expression is the first named child (after `@`)
    let expr = decorator.named_child(0)?;

    match expr.kind() {
        // @Get, @Controller (no args) — plain identifier
        "identifier" => {
            let name = node_text(&expr, source);
            Some((name, None, line))
        }
        // @Get(':slug'), @Controller('articles'), @UseGuards(AuthGuard)
        "call_expression" => {
            let fn_node = expr.child_by_field_name("function")?;
            let name = node_text(&fn_node, source);

            // Extract first argument — try as string first, fallback to raw text
            let first_arg = expr
                .child_by_field_name("arguments")
                .and_then(|args| {
                    let arg_list = collect_arguments(&args, source);
                    arg_list.into_iter().next()
                })
                .map(|arg| extract_string_value(&arg.text).unwrap_or(arg.text));

            Some((name, first_arg, line))
        }
        _ => None,
    }
}

/// Detect auth from NestJS `@UseGuards(...)` decorator.
fn detect_nestjs_auth(decorators: &[(String, Option<String>, usize)]) -> Option<AuthKind> {
    for (name, arg, _) in decorators {
        if name == "UseGuards" {
            let guard_detail = arg.as_deref().unwrap_or("AuthGuard");
            return Some(AuthKind::Decorator(format!("UseGuards({guard_detail})")));
        }
    }
    None
}

/// Compose a NestJS route path from controller prefix and method subpath.
///
/// `@Controller('articles')` + `@Get(':slug')` → `/articles/:slug`
/// `@Controller('articles')` + `@Get()` → `/articles`
fn compose_nestjs_path(prefix: &str, subpath: Option<&str>) -> String {
    let prefix = prefix.trim_matches('/');
    let sub = subpath.unwrap_or("").trim_matches('/');

    if prefix.is_empty() && sub.is_empty() {
        "/".to_string()
    } else if prefix.is_empty() {
        format!("/{sub}")
    } else if sub.is_empty() {
        format!("/{prefix}")
    } else {
        format!("/{prefix}/{sub}")
    }
}

/// Find a direct child node by its kind.
fn find_child_by_kind<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == kind {
                return Some(child);
            }
        }
    }
    None
}

fn collect_import_specifiers(node: &Node, source: &str, specifiers: &mut Vec<String>) {
    match node.kind() {
        "identifier" => {
            let text = node_text_ref(node, source);
            if text != "from" && text != "import" && text != "as" {
                specifiers.push(text.to_string());
            }
        }
        "import_specifier" => {
            if let Some(name) = node.child_by_field_name("name") {
                specifiers.push(node_text(&name, source));
            }
        }
        "namespace_import" => {
            specifiers.push(node_text(node, source));
        }
        _ => {
            let child_count = node.child_count();
            for i in 0..child_count {
                if let Some(child) = node.child(i as u32) {
                    collect_import_specifiers(&child, source, specifiers);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::tree_sitter;

    fn extract_ts(source: &str) -> FileExtraction {
        let path = PathBuf::from("test.ts");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::TypeScript, None)
                .unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::TypeScript)
    }

    #[test]
    fn extracts_express_get_route() {
        let ext = extract_ts(
            r#"
import express from 'express';
const app = express();
app.get('/api/users', (req, res) => res.json([]));
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/users");
    }

    #[test]
    fn extracts_express_post_route() {
        let ext = extract_ts(
            r#"
const router = require('express').Router();
router.post('/api/items', (req, res) => res.status(201).json({}));
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
    }

    #[test]
    fn detects_auth_middleware_on_route() {
        let ext = extract_ts(
            r#"
app.post('/api/orders', authMiddleware, (req, res) => {
    res.json({ ok: true });
});
"#,
        );
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Middleware("authMiddleware".into()))
        );
    }

    #[test]
    fn detects_jwt_middleware() {
        let ext = extract_ts(
            r#"
app.delete('/api/users/:id', verifyJwt, validateAdmin, (req, res) => {
    res.status(204).send();
});
"#,
        );
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn extracts_fetch_http_call() {
        let ext = extract_ts(
            r#"
const resp = await fetch("https://api.example.com/users");
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn extracts_axios_http_call() {
        let ext = extract_ts(r#"const data = await axios.get("https://payment.service/charge");"#);
        assert_eq!(ext.dependencies.len(), 1);
    }

    #[test]
    fn extracts_console_log_sinks() {
        let ext = extract_ts(
            r#"
console.log("Server started");
console.error("Something failed");
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
        assert!(!ext.sinks[0].contains_pii);
    }

    #[test]
    fn extracts_logger_sinks() {
        let ext = extract_ts(
            r#"
logger.info("Request processed");
logger.warn("Slow query");
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_ts(r#"console.log("User email:", user.email);"#);
        assert!(ext.sinks[0].contains_pii);
    }

    #[test]
    fn detects_password_pii() {
        let ext = extract_ts(r#"console.log("Login with password:", password);"#);
        assert!(ext.sinks[0].contains_pii);
    }

    #[test]
    fn extracts_imports() {
        let ext = extract_ts(
            r#"
import express from 'express';
import { Router, Request } from 'express';
"#,
        );
        assert_eq!(ext.imports.len(), 2);
        assert_eq!(ext.imports[0].source, "express");
    }

    #[test]
    fn works_with_javascript_grammar() {
        let path = PathBuf::from("app.js");
        let source = r#"
const express = require('express');
const app = express();
app.get('/api/data', (req, res) => {
    console.log("request received");
    res.json({ ok: true });
});
"#;
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::JavaScript, None)
                .unwrap();
        let ext = extract(&path, source, &parsed.tree, SupportedLanguage::JavaScript);

        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.sinks.len(), 1);
        assert_eq!(ext.language, SupportedLanguage::JavaScript);
    }

    // --- NestJS ---

    #[test]
    fn extracts_nestjs_get_route() {
        let ext = extract_ts(
            r#"
@Controller('articles')
class ArticlesController {
    @Get()
    findAll() {
        return [];
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/articles");
    }

    #[test]
    fn extracts_nestjs_get_with_subpath() {
        let ext = extract_ts(
            r#"
@Controller('articles')
class ArticlesController {
    @Get(':slug')
    findOne() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/articles/:slug");
    }

    #[test]
    fn extracts_nestjs_post_route() {
        let ext = extract_ts(
            r#"
@Controller('users')
class UsersController {
    @Post()
    create() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn detects_nestjs_useguards_on_method() {
        let ext = extract_ts(
            r#"
@Controller('items')
class ItemsController {
    @Post()
    @UseGuards(AuthGuard('jwt'))
    create() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
        match &ext.interfaces[0].auth {
            Some(AuthKind::Decorator(s)) => assert!(s.contains("UseGuards")),
            other => panic!("expected Decorator auth, got {:?}", other),
        }
    }

    #[test]
    fn detects_nestjs_useguards_on_class() {
        let ext = extract_ts(
            r#"
@Controller('admin')
@UseGuards(AuthGuard('jwt'))
class AdminController {
    @Get()
    dashboard() {
        return {};
    }

    @Delete(':id')
    remove() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 2);
        // Class-level UseGuards applies to all methods
        assert!(ext.interfaces.iter().all(|i| i.auth.is_some()));
    }

    #[test]
    fn nestjs_method_auth_overrides_class() {
        let ext = extract_ts(
            r#"
@Controller('mixed')
@UseGuards(AuthGuard('basic'))
class MixedController {
    @Get()
    @UseGuards(AuthGuard('jwt'))
    secured() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        match &ext.interfaces[0].auth {
            Some(AuthKind::Decorator(s)) => assert!(s.contains("jwt")),
            other => panic!("expected jwt auth, got {:?}", other),
        }
    }

    #[test]
    fn realistic_nestjs_controller() {
        let ext = extract_ts(
            r#"
import { Controller, Get, Post, Delete, UseGuards } from '@nestjs/common';

@Controller('api/articles')
@UseGuards(AuthGuard('jwt'))
class ArticlesController {
    @Get()
    findAll() {
        console.log("Listing articles");
        return [];
    }

    @Get(':slug')
    findOne() {
        return {};
    }

    @Post()
    create() {
        console.log("Creating article for:", user.email);
        return {};
    }

    @Delete(':slug')
    remove() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 4, "4 NestJS routes");
        assert!(
            ext.interfaces.iter().all(|i| i.auth.is_some()),
            "all should inherit class auth"
        );
        assert_eq!(ext.interfaces[0].path, "/api/articles");
        assert_eq!(ext.interfaces[1].path, "/api/articles/:slug");
        assert!(ext.sinks.len() >= 1, "should detect console.log sinks");
        assert!(ext.imports.len() >= 1, "should detect imports");
    }

    #[test]
    fn nestjs_coexists_with_express() {
        let ext = extract_ts(
            r#"
import express from 'express';

const app = express();
app.get('/health', (req, res) => res.json({ ok: true }));

@Controller('api/users')
class UsersController {
    @Get()
    findAll() {
        return [];
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 2, "1 Express + 1 NestJS");
        assert!(
            ext.interfaces.iter().any(|i| i.path == "/health"),
            "Express route"
        );
        assert!(
            ext.interfaces.iter().any(|i| i.path == "/api/users"),
            "NestJS route"
        );
    }

    #[test]
    fn realistic_multi_feature_file() {
        let ext = extract_ts(
            r#"
import express from 'express';
import { authMiddleware } from './auth';

const app = express();

app.get('/health', (req, res) => {
    res.json({ status: 'ok' });
});

app.post('/api/payments', authMiddleware, async (req, res) => {
    console.log("Processing payment for:", req.body.email);
    const result = await fetch("https://payment.gateway/charge", {
        method: 'POST',
        body: JSON.stringify(req.body),
    });
    logger.info("Payment processed");
    res.json(await result.json());
});

app.get('/api/users', (req, res) => {
    console.log("Fetching users");
    res.json([]);
});
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        assert!(
            ext.interfaces
                .iter()
                .find(|i| i.path == "/health")
                .unwrap()
                .auth
                .is_none()
        );
        assert!(
            ext.interfaces
                .iter()
                .find(|i| i.path == "/api/payments")
                .unwrap()
                .auth
                .is_some()
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(ext.sinks.len(), 3);
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
        assert_eq!(ext.imports.len(), 2);
    }
}
