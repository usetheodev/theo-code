//! Python semantic extraction from tree-sitter CSTs.
//!
//! Handles the Python language family, covering three major frameworks:
//! - **FastAPI**: `@app.get("/path")`, `@router.post("/path")`
//! - **Flask**: `@app.route("/path")`, `@app.get("/path")` (Flask 2.0+)
//! - **Django**: `path("url/", views.handler)` in URL patterns
//!
//! Extracts:
//! - Route definitions from decorators and URL pattern calls
//! - Auth decorator detection (`@login_required`, `@jwt_required`, etc.)
//! - External HTTP calls (requests, httpx)
//! - Log sinks with PII detection

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::patterns;
use crate::tree_sitter::SupportedLanguage;
use crate::types::*;

use super::common::{
    self, anchor_from_node, extract_string_value, node_text, node_text_ref, truncate_call_text,
};

/// Extract semantic information from a Python source file.
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
        "decorated_definition" => {
            try_extract_decorated_route(node, source, file_path, extraction);
        }
        "call" => {
            try_extract_django_path(node, source, file_path, extraction);
            try_extract_http_call(node, source, file_path, extraction);
            common::try_extract_log_sink(node, source, file_path, extraction);
        }
        "import_statement" => {
            try_extract_import_statement(node, source, extraction);
        }
        "import_from_statement" => {
            try_extract_import_from_statement(node, source, extraction);
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

// ---------------------------------------------------------------------------
// Import extraction
// ---------------------------------------------------------------------------

/// Extract import info from `import os` or `import torch.nn as nn`.
///
/// Python's `import_statement` AST:
/// ```text
/// (import_statement
///   name: (dotted_name)          ; "os" or "torch.nn"
///   alias: (aliased_import       ; optional "as nn"
///     name: (dotted_name)
///     alias: (identifier)))
/// ```
fn try_extract_import_statement(node: &Node, source: &str, extraction: &mut FileExtraction) {
    let line = node.start_position().row + 1;

    for i in 0..node.named_child_count() {
        let child = match node.named_child(i as u32) {
            Some(c) => c,
            None => continue,
        };

        let (module_name, alias) = match child.kind() {
            "dotted_name" | "identifier" => (node_text_ref(&child, source).to_string(), None),
            "aliased_import" => {
                // `import torch.nn as nn` — name is first child, alias is second
                let name = match child.named_child(0) {
                    Some(name_node) => node_text_ref(&name_node, source).to_string(),
                    None => continue,
                };
                let alias_name = child
                    .named_child(1)
                    .map(|a| node_text_ref(&a, source).to_string());
                (name, alias_name)
            }
            _ => continue,
        };

        if module_name.is_empty() {
            continue;
        }

        // `import torch.nn` → source = "torch.nn", specifier = "torch.nn"
        // `import torch.nn as nn` → source = "torch.nn", specifier = "torch.nn", alias "nn" → "torch.nn"
        let aliases = match alias {
            Some(alias_name) => vec![(alias_name, module_name.clone())],
            None => vec![],
        };

        extraction.imports.push(ImportInfo {
            source: module_name.clone(),
            specifiers: vec![module_name],
            line,
            aliases,
        });
    }
}

/// Extract import info from `from fastapi import FastAPI, Depends`.
///
/// Python's `import_from_statement` AST:
/// ```text
/// (import_from_statement
///   module_name: (dotted_name)   ; "fastapi" or (relative_import)
///   name: (dotted_name)          ; "FastAPI", repeated for each specifier
///   ...)
/// ```
///
/// Handles:
/// - Absolute: `from fastapi import FastAPI` → source "fastapi"
/// - Relative: `from . import views` → source "."
/// - Relative with module: `from ..utils import helper` → source "..utils"
/// - Wildcard: `from os.path import *` → source "os.path", specifier "*"
fn try_extract_import_from_statement(node: &Node, source: &str, extraction: &mut FileExtraction) {
    let line = node.start_position().row + 1;

    // Build the module source from the node text.
    // The module_name field gives the module after `from`.
    // For relative imports, we also need to capture the dots.
    let module_source = build_python_import_source(node, source);
    if module_source.is_empty() {
        return;
    }

    // Collect imported specifiers and aliases
    let mut specifiers = Vec::new();
    let mut aliases = Vec::new();
    for i in 0..node.named_child_count() {
        let child = match node.named_child(i as u32) {
            Some(c) => c,
            None => continue,
        };

        match child.kind() {
            "dotted_name" | "identifier" => {
                // Skip the module_name — it's the source, not a specifier.
                // The module_name is typically the first named child.
                // Specifiers come after the "import" keyword.
                // We use a heuristic: if this child's start is after the "import" keyword, it's a specifier.
                let text = node_text_ref(&child, source);
                if text != module_source && !module_source.ends_with(text) {
                    specifiers.push(text.to_string());
                }
            }
            "aliased_import" => {
                // `from x import Foo as Bar` — specifier is "Foo", alias "Bar" → "Foo"
                if let Some(name_node) = child.named_child(0) {
                    let original = node_text_ref(&name_node, source).to_string();
                    specifiers.push(original.clone());
                    if let Some(alias_node) = child.named_child(1) {
                        let alias_name = node_text_ref(&alias_node, source).to_string();
                        aliases.push((alias_name, original));
                    }
                }
            }
            "wildcard_import" => {
                specifiers.push("*".to_string());
            }
            _ => {}
        }
    }

    // If no specifiers were found (shouldn't happen in valid Python), use the module as specifier
    if specifiers.is_empty() {
        specifiers.push(module_source.clone());
    }

    extraction.imports.push(ImportInfo {
        source: module_source,
        specifiers,
        line,
        aliases,
    });
}

/// Build the module source string for a Python `import_from_statement`.
///
/// Handles:
/// - `from fastapi import X` → "fastapi"
/// - `from . import X` → "."
/// - `from ..utils import X` → "..utils"
/// - `from torch.nn.modules import X` → "torch.nn.modules"
fn build_python_import_source(node: &Node, source: &str) -> String {
    // Walk through children to find the module part (between "from" and "import").
    // For relative imports, we need to capture the dots + optional module name.
    let full_text = node_text_ref(node, source);

    // Find "from " and "import " boundaries
    let from_end = match full_text.find("from") {
        Some(pos) => pos + 4,
        None => return String::new(),
    };
    let import_start = match full_text.find(" import ") {
        Some(pos) => pos,
        None => return String::new(),
    };

    if import_start <= from_end {
        return String::new();
    }

    // The module part is between "from " and " import "
    let module_part = full_text[from_end..import_start].trim();
    module_part.to_string()
}

/// Try to extract a route from a decorated function definition.
///
/// Handles FastAPI and Flask patterns:
/// ```python
/// @app.get("/users")         # FastAPI
/// @router.post("/orders")    # FastAPI
/// @app.route("/items")       # Flask
/// @login_required            # Auth decorator
/// def handler(): ...
/// ```
fn try_extract_decorated_route(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let mut route_info: Option<(HttpMethod, String, SourceAnchor)> = None;
    let mut auth: Option<AuthKind> = None;

    // Iterate over decorator children
    for i in 0..node.child_count() {
        let child = match node.child(i as u32) {
            Some(c) if c.kind() == "decorator" => c,
            _ => continue,
        };

        // The decorator expression is the first named child (after @ token)
        let expr = match child.named_child(0) {
            Some(e) => e,
            None => continue,
        };

        // Check for route decorator
        if let Some((method, path)) = try_parse_route_decorator(&expr, source) {
            route_info = Some((method, path, anchor_from_node(&child, file_path)));
        }

        // Check for auth decorator
        if auth.is_none()
            && let Some(auth_kind) = try_parse_auth_decorator(&expr, source) {
                auth = Some(auth_kind);
            }
    }

    if let Some((method, path, anchor)) = route_info {
        // Extract the function name from the decorated_definition's function_definition child
        let handler_name = (0..node.child_count())
            .filter_map(|i| node.child(i as u32))
            .find(|child| child.kind() == "function_definition")
            .and_then(|func_def| func_def.child_by_field_name("name"))
            .map(|name_node| node_text_ref(&name_node, source).to_string());

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

/// Parse a decorator expression to extract route information.
///
/// Returns `Some((method, path))` for route decorators like:
/// - `@app.get("/users")` → (Get, "/users")
/// - `@app.route("/items")` → (All, "/items")
///
/// Filters out false positives from non-router decorators such as
/// `@patch("module.path")` from `unittest.mock` by:
/// 1. Requiring the object to be a known router/app variable name
/// 2. Requiring the path argument to start with `/`
fn try_parse_route_decorator(expr: &Node, source: &str) -> Option<(HttpMethod, String)> {
    // Route decorators are always calls: @app.get("/path")
    if expr.kind() != "call" {
        return None;
    }

    let function = expr.child_by_field_name("function")?;

    // Must be an attribute access: app.get, router.post, etc.
    if function.kind() != "attribute" {
        return None;
    }

    // Validate the object is a known router/app variable.
    // Without this, @patch("...") from unittest.mock matches as HTTP PATCH.
    let object = function.child_by_field_name("object")?;
    let obj_text = node_text_ref(&object, source);
    if !is_route_object(obj_text) {
        return None;
    }

    let method_name = node_text_ref(&function.child_by_field_name("attribute")?, source);

    // Determine HTTP method from decorator name
    let http_method = if method_name == "route" {
        // Flask's @app.route() — defaults to ALL
        HttpMethod::All
    } else {
        common::parse_http_method(method_name)?
    };

    // Extract path from first argument
    let args = expr.child_by_field_name("arguments")?;
    let first_arg = find_first_string_arg(&args, source)?;

    // Route paths must start with "/" — dotted module paths like
    // "torch._C._func" are mock targets, not HTTP endpoints.
    if !first_arg.starts_with('/') {
        return None;
    }

    Some((http_method, first_arg))
}

/// Known variable names for Python web framework router/app objects.
///
/// Covers FastAPI, Flask, and common naming conventions.
/// Intentionally conservative — better to miss an exotic alias than
/// to produce false positives from unrelated libraries.
fn is_route_object(name: &str) -> bool {
    matches!(
        name,
        "app" | "router" | "api" | "blueprint" | "bp" | "api_router" | "web" | "route"
    )
}

/// Check if a string looks like a file system path rather than a URL pattern.
///
/// File paths contain extensions like `.py`, `.sh`, `.js` etc.
/// Django URL patterns use segments like `users/`, `<int:id>/`, `{param}`.
fn looks_like_file_path(s: &str) -> bool {
    let extensions = [
        ".py", ".sh", ".js", ".ts", ".jsx", ".tsx", ".rb", ".go", ".rs", ".java", ".cs", ".php",
        ".c", ".cpp", ".h", ".hpp", ".swift", ".kt", ".scala", ".sql", ".html", ".css", ".json",
        ".yaml", ".yml", ".toml", ".xml", ".txt", ".md", ".cfg", ".ini", ".conf", ".log", ".csv",
    ];
    let lower = s.to_lowercase();
    extensions.iter().any(|ext| lower.ends_with(ext))
}

/// Parse a decorator expression to detect auth indicators.
///
/// Handles:
/// - `@login_required` (bare identifier)
/// - `@jwt_required()` (call with no args)
/// - `@permission_classes([IsAuthenticated])` (call with args)
fn try_parse_auth_decorator(expr: &Node, source: &str) -> Option<AuthKind> {
    let name = match expr.kind() {
        "identifier" => node_text_ref(expr, source),
        "call" => {
            let function = expr.child_by_field_name("function")?;
            match function.kind() {
                "identifier" => node_text_ref(&function, source),
                "attribute" => node_text_ref(&function.child_by_field_name("attribute")?, source),
                _ => return None,
            }
        }
        "attribute" => node_text_ref(&expr.child_by_field_name("attribute")?, source),
        _ => return None,
    };

    if patterns::is_auth_indicator(name) {
        Some(AuthKind::Decorator(name.to_string()))
    } else {
        None
    }
}

/// Try to extract a Django URL pattern from `path("url/", view)`.
///
/// Requires at least two positional arguments (URL pattern + view) to
/// distinguish Django's `path("users/", views.list)` from unrelated
/// functions like `path("bin/script.py")`.
///
/// Also rejects paths containing file extensions (`.py`, `.sh`, etc.)
/// which are file system paths, not URL patterns.
fn try_extract_django_path(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let function = match node.child_by_field_name("function") {
        Some(f) => f,
        None => return,
    };

    // Match `path(...)` or `re_path(...)`
    let func_name = match function.kind() {
        "identifier" => node_text_ref(&function, source),
        _ => return,
    };

    if func_name != "path" && func_name != "re_path" {
        return;
    }

    let args = match node.child_by_field_name("arguments") {
        Some(a) => a,
        None => return,
    };

    // Django's path() requires at least 2 positional args: URL pattern + view.
    // A bare `path("something")` with one arg is likely a file path helper.
    let positional_count = (0..args.named_child_count())
        .filter_map(|i| args.named_child(i as u32))
        .filter(|c| c.kind() != "keyword_argument")
        .count();
    if positional_count < 2 {
        return;
    }

    let url_path = match find_first_string_arg(&args, source) {
        Some(p) => {
            // Reject file system paths: strings containing file extensions
            // like ".py", ".sh", ".js" are not Django URL patterns.
            if looks_like_file_path(&p) {
                return;
            }
            // Django paths don't start with / — normalize
            if p.starts_with('/') {
                p
            } else {
                format!("/{p}")
            }
        }
        None => return,
    };

    // Extract handler name from the second positional argument.
    // Django patterns: path('url/', views.handler_name) or path('url/', handler_name)
    let handler_name = args.named_child(1).and_then(|second_arg| {
        // Skip keyword arguments
        if second_arg.kind() == "keyword_argument" {
            return None;
        }
        let text = node_text_ref(&second_arg, source);
        // For dotted references like "views.list_users", take the last segment
        if let Some(last_dot) = text.rfind('.') {
            Some(text[last_dot + 1..].to_string())
        } else if !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == '_') {
            // Simple identifier reference
            Some(text.to_string())
        } else {
            None
        }
    });

    extraction.interfaces.push(Interface {
        method: HttpMethod::All,
        path: url_path.clone(),
        auth: None,
        anchor: anchor_from_node(node, file_path),
        parameters: common::extract_path_params(&url_path),
        handler_name,
        request_body_type: None,
    });
}

/// Try to extract an HTTP call from `requests.get(url)` or `httpx.post(url)`.
fn try_extract_http_call(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    let function = match node.child_by_field_name("function") {
        Some(f) if f.kind() == "attribute" => f,
        _ => return,
    };

    let object_name = match function.child_by_field_name("object") {
        Some(obj) if obj.kind() == "identifier" => node_text_ref(&obj, source),
        _ => return,
    };

    let method_name = match function.child_by_field_name("attribute") {
        Some(attr) => node_text_ref(&attr, source),
        None => return,
    };

    // Known HTTP client libraries
    let is_http_client = matches!(object_name, "requests" | "httpx")
        && matches!(
            method_name,
            "get" | "post" | "put" | "patch" | "delete" | "head" | "options"
        );

    if !is_http_client {
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

/// Find the first string literal argument in an argument list.
fn find_first_string_arg(args_node: &Node, source: &str) -> Option<String> {
    for i in 0..args_node.named_child_count() {
        if let Some(child) = args_node.named_child(i as u32) {
            // Skip keyword arguments — we want positional
            if child.kind() == "keyword_argument" {
                continue;
            }
            let text = node_text(&child, source);
            if let Some(value) = extract_string_value(&text) {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;
