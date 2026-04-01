//! Shared utilities used across all language-specific extractors.
//!
//! These functions extract text from tree-sitter nodes, parse arguments,
//! detect log sinks, and provide other cross-cutting helpers. By
//! centralizing them here we keep each extractor focused on its
//! framework-specific CST walking logic.

use std::path::Path;

use tree_sitter::Node;

use crate::patterns;
use crate::types::*;

/// Create a [`SourceAnchor`] from a tree-sitter node and file path.
///
/// Captures the full node position: start/end line, byte offsets, and
/// CST node kind. This is the standard way to create anchors inside
/// extractors — every `Interface`, `Dependency`, `Sink`, `Symbol`,
/// and `DataModel` construction should use this.
pub fn anchor_from_node(node: &Node, file_path: &Path) -> SourceAnchor {
    SourceAnchor {
        file: file_path.to_path_buf(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        node_kind: node.kind().to_string(),
    }
}

/// Extract the source text spanned by a tree-sitter node.
pub fn node_text(node: &Node, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_string()
}

/// Extract the source text spanned by a tree-sitter node as a borrowed slice.
///
/// Zero-copy alternative to [`node_text`] — returns a `&str` tied to the
/// source lifetime instead of allocating a new `String`. Use this when
/// the text is only needed for comparisons, pattern matching, or other
/// read-only operations within the same scope.
pub fn node_text_ref<'a>(node: &Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// Strip surrounding quote characters (single, double, or backtick).
pub fn strip_quotes(s: &str) -> String {
    s.trim_matches(|c| c == '\'' || c == '"' || c == '`')
        .to_string()
}

/// Strip surrounding quote characters without allocating.
///
/// Zero-copy alternative to [`strip_quotes`] — returns a `&str` slice
/// of the input with leading/trailing quote characters removed.
pub fn strip_quotes_ref(s: &str) -> &str {
    s.trim_matches(|c| c == '\'' || c == '"' || c == '`')
}

/// Try to parse a string literal from an argument's text.
///
/// Returns `Some(unquoted_value)` if the text is a quoted string,
/// `None` otherwise.
pub fn extract_string_value(text: &str) -> Option<String> {
    let text = text.trim();
    if (text.starts_with('\'') && text.ends_with('\''))
        || (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('`') && text.ends_with('`'))
    {
        Some(strip_quotes_ref(text).to_string())
    } else {
        None
    }
}

/// A single argument extracted from a call's argument list.
pub struct ArgumentInfo {
    pub text: String,
}

/// Collect all non-punctuation children of an argument list node.
///
/// Skips `(`, `)`, and `,` tokens, returning the remaining children
/// as `ArgumentInfo` values with their source text.
pub fn collect_arguments(args_node: &Node, source: &str) -> Vec<ArgumentInfo> {
    let mut result = Vec::new();
    let count = args_node.child_count();
    for i in 0..count {
        if let Some(child) = args_node.child(i as u32) {
            if child.kind() == "(" || child.kind() == ")" || child.kind() == "," {
                continue;
            }
            result.push(ArgumentInfo {
                text: node_text(&child, source),
            });
        }
    }
    result
}

/// Parse an HTTP method name (case-insensitive) into an `HttpMethod`.
pub fn parse_http_method(name: &str) -> Option<HttpMethod> {
    match name.to_lowercase().as_str() {
        "get" => Some(HttpMethod::Get),
        "post" => Some(HttpMethod::Post),
        "put" => Some(HttpMethod::Put),
        "patch" => Some(HttpMethod::Patch),
        "delete" => Some(HttpMethod::Delete),
        "options" => Some(HttpMethod::Options),
        "head" => Some(HttpMethod::Head),
        "all" => Some(HttpMethod::All),
        _ => None,
    }
}

/// Check if a CST node kind represents a function/method call.
///
/// Each language family uses different node names:
/// - JS/TS/Go/Rust/C/C++/Swift/Scala: `call_expression`
/// - Python/Ruby: `call`
/// - Java/Kotlin: `method_invocation`
/// - C#: `invocation_expression`
/// - PHP: `member_call_expression`, `function_call_expression`, `scoped_call_expression`
/// - Ruby: `method_call`
#[deprecated(note = "Use LanguageBehavior::call_node_kinds() for language-specific call detection")]
pub fn is_call_node(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression"
            | "call"
            | "method_invocation"
            | "invocation_expression"
            | "member_call_expression"
            | "function_call_expression"
            | "scoped_call_expression"
            | "method_call"
    )
}

/// Try to detect a log sink using text-based heuristic matching.
///
/// Checks if the call text contains an `object.method(` pattern where
/// `object` is a known log object and `method` is a known log method.
/// This is the generic detection shared by all extractors — framework-
/// specific extractors may also use their own CST-aware detection.
pub fn try_extract_log_sink(
    node: &Node,
    source: &str,
    file_path: &Path,
    extraction: &mut FileExtraction,
) {
    // Quick exit using zero-copy text: does the call text contain any log object name?
    let call_ref = node_text_ref(node, source);
    let call_lower = call_ref.to_lowercase();
    let has_log_object = patterns::LOG_OBJECTS
        .iter()
        .any(|obj| call_lower.contains(&obj.to_lowercase()));

    if !has_log_object {
        return;
    }

    // Check for object.method( and object::method( patterns
    // The `::` variant covers PHP (Log::info) and C++ (Logger::error)
    for obj in patterns::LOG_OBJECTS {
        for method in patterns::LOG_METHODS {
            let dot_pattern = format!("{obj}.{method}(");
            let scope_pattern = format!("{obj}::{method}(");
            if call_ref.contains(&dot_pattern) || call_ref.contains(&scope_pattern) {
                let pii = patterns::contains_pii(call_ref);
                extraction.sinks.push(Sink {
                    sink_type: SinkType::Log,
                    anchor: anchor_from_node(node, file_path),
                    text: call_ref.to_string(),
                    contains_pii: pii,
                });
                return;
            }
        }
    }
}

/// Extract route parameters from a URL path pattern.
///
/// Recognizes three common styles used across web frameworks:
/// - `:param` — Express, Gin, Echo, Rails
/// - `{param}` — FastAPI, Spring, ASP.NET, Laravel, OpenAPI
/// - `<param>` or `<type:param>` — Flask
///
/// Returns a `RouteParameter` for each unique parameter found, all with
/// `location: ParameterLocation::Path` and `param_type: None`.
pub fn extract_path_params(route_path: &str) -> Vec<RouteParameter> {
    let mut params = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Style 1: `:param` — matches :word characters after / or at start
    // Must not be inside {} or <> to avoid double-extraction
    for segment in route_path.split('/') {
        if let Some(name) = segment.strip_prefix(':') {
            // Strip optional trailing characters like (regex) in Express
            let name = name.split('(').next().unwrap_or(name);
            if !name.is_empty() && seen.insert(name.to_string()) {
                params.push(RouteParameter {
                    name: name.to_string(),
                    location: ParameterLocation::Path,
                    param_type: None,
                });
            }
        }
    }

    // Style 2: `{param}` — used by FastAPI, Spring, ASP.NET, Laravel
    let mut rest = route_path;
    while let Some(start) = rest.find('{') {
        if let Some(end) = rest[start..].find('}') {
            let inner = &rest[start + 1..start + end];
            // Handle {param:regex} (ASP.NET constraints) — take only the name
            let name = inner.split(':').next().unwrap_or(inner).trim();
            if !name.is_empty() && !name.contains(' ') && seen.insert(name.to_string()) {
                params.push(RouteParameter {
                    name: name.to_string(),
                    location: ParameterLocation::Path,
                    param_type: None,
                });
            }
            rest = &rest[start + end + 1..];
        } else {
            break;
        }
    }

    // Style 3: `<param>` or `<type:param>` — Flask style
    rest = route_path;
    while let Some(start) = rest.find('<') {
        if let Some(end) = rest[start..].find('>') {
            let inner = &rest[start + 1..start + end];
            // Flask uses <type:name>, take the last part after ':'
            let name = if inner.contains(':') {
                inner.rsplit(':').next().unwrap_or(inner)
            } else {
                inner
            };
            let name = name.trim();
            if !name.is_empty() && !name.contains(' ') && seen.insert(name.to_string()) {
                params.push(RouteParameter {
                    name: name.to_string(),
                    location: ParameterLocation::Path,
                    param_type: None,
                });
            }
            rest = &rest[start + end + 1..];
        } else {
            break;
        }
    }

    params
}

/// Truncate a call expression text to a maximum display length.
pub fn truncate_call_text(text: String, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", &text[..max_len.saturating_sub(3)])
    } else {
        text
    }
}

/// Create a new empty `FileExtraction` for the given file and language.
pub fn new_extraction(
    file_path: &Path,
    language: crate::tree_sitter::SupportedLanguage,
) -> FileExtraction {
    FileExtraction {
        file: file_path.to_path_buf(),
        language,
        interfaces: Vec::new(),
        dependencies: Vec::new(),
        sinks: Vec::new(),
        imports: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        data_models: Vec::new(),
        env_dependencies: Vec::new(),
        file_role: FileRole::Implementation,
        estimated_tokens: 0,
        content_hash: None,
        git_metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_call_text_works() {
        assert_eq!(truncate_call_text("short".into(), 100), "short");
        let long = "a".repeat(120);
        let truncated = truncate_call_text(long, 100);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 100);
    }

    #[test]
    fn strip_quotes_removes_all_quote_types() {
        assert_eq!(strip_quotes("'hello'"), "hello");
        assert_eq!(strip_quotes("\"hello\""), "hello");
        assert_eq!(strip_quotes("`hello`"), "hello");
        assert_eq!(strip_quotes("no_quotes"), "no_quotes");
        assert_eq!(strip_quotes("''"), "");
    }

    #[test]
    fn extract_string_value_from_quoted_text() {
        assert_eq!(
            extract_string_value("'/api/users'"),
            Some("/api/users".into())
        );
        assert_eq!(
            extract_string_value("\"/api/users\""),
            Some("/api/users".into())
        );
        assert_eq!(
            extract_string_value("`/api/users`"),
            Some("/api/users".into())
        );
        assert_eq!(extract_string_value("variable"), None);
        assert_eq!(extract_string_value("123"), None);
        assert_eq!(
            extract_string_value("  '/spaced'  "),
            Some("/spaced".into())
        );
    }

    #[test]
    fn parse_http_method_case_insensitive() {
        assert_eq!(parse_http_method("get"), Some(HttpMethod::Get));
        assert_eq!(parse_http_method("GET"), Some(HttpMethod::Get));
        assert_eq!(parse_http_method("Get"), Some(HttpMethod::Get));
        assert_eq!(parse_http_method("post"), Some(HttpMethod::Post));
        assert_eq!(parse_http_method("POST"), Some(HttpMethod::Post));
        assert_eq!(parse_http_method("put"), Some(HttpMethod::Put));
        assert_eq!(parse_http_method("delete"), Some(HttpMethod::Delete));
        assert_eq!(parse_http_method("patch"), Some(HttpMethod::Patch));
        assert_eq!(parse_http_method("options"), Some(HttpMethod::Options));
        assert_eq!(parse_http_method("head"), Some(HttpMethod::Head));
        assert_eq!(parse_http_method("all"), Some(HttpMethod::All));
        assert_eq!(parse_http_method("unknown"), None);
        assert_eq!(parse_http_method(""), None);
    }

    #[test]
    fn extract_path_params_colon_style() {
        let params = extract_path_params("/users/:id/posts/:postId");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "id");
        assert_eq!(params[0].location, ParameterLocation::Path);
        assert!(params[0].param_type.is_none());
        assert_eq!(params[1].name, "postId");
    }

    #[test]
    fn extract_path_params_curly_style() {
        let params = extract_path_params("/users/{id}");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "id");
    }

    #[test]
    fn extract_path_params_curly_with_constraint() {
        // ASP.NET style: {id:int}
        let params = extract_path_params("/users/{id:int}/orders/{orderId:guid}");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "id");
        assert_eq!(params[1].name, "orderId");
    }

    #[test]
    fn extract_path_params_flask_angle_style() {
        let params = extract_path_params("/files/<path:filename>");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "filename");
    }

    #[test]
    fn extract_path_params_flask_simple() {
        let params = extract_path_params("/users/<id>");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "id");
    }

    #[test]
    fn extract_path_params_no_params() {
        let params = extract_path_params("/health");
        assert!(params.is_empty());
    }

    #[test]
    fn extract_path_params_no_duplicates() {
        // If somehow same param name appears twice, only one should be returned
        let params = extract_path_params("/users/:id/alias/:id");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn extract_path_params_mixed_styles_no_double_extract() {
        // Each param should only be extracted once even with mixed formats
        let params = extract_path_params("/users/{userId}/posts/:postId");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "postId"); // colon style first
        assert_eq!(params[1].name, "userId"); // curly style second
    }

    #[test]
    #[allow(deprecated)]
    fn is_call_node_matches_all_language_variants() {
        assert!(is_call_node("call_expression"));
        assert!(is_call_node("call"));
        assert!(is_call_node("method_invocation"));
        assert!(is_call_node("invocation_expression"));
        assert!(is_call_node("member_call_expression"));
        assert!(is_call_node("function_call_expression"));
        assert!(is_call_node("scoped_call_expression"));
        assert!(is_call_node("method_call"));
        assert!(!is_call_node("identifier"));
        assert!(!is_call_node("expression_statement"));
    }
}
