//! Environment variable detection across languages.
//!
//! Detects references to environment variables in source code via recursive
//! CST traversal with language-specific pattern matching. Each detected
//! reference produces an [`EnvDependency`] with the variable name and
//! source location.
//!
//! Supported patterns:
//! - **TypeScript/JS:** `process.env.VAR`, `import.meta.env.VAR`
//! - **Python:** `os.getenv("VAR")`, `os.environ["VAR"]`, `os.environ.get("VAR")`
//! - **Go:** `os.Getenv("VAR")`
//! - **Java:** `System.getenv("VAR")`, `@Value("${VAR}")`
//! - **C#:** `Environment.GetEnvironmentVariable("VAR")`
//! - **PHP:** `getenv("VAR")`, `env("VAR")`, `$_ENV["VAR"]`
//! - **Ruby:** `ENV["VAR"]`, `ENV.fetch("VAR")`
//! - **Rust:** `std::env::var("VAR")`, `env::var("VAR")`, `env!("VAR")`
//!
//! Dynamic access (e.g., `process.env[varName]`) produces `var_name: "<dynamic>"`.

use std::path::Path;

use tree_sitter::{Node, Tree};

use crate::types::EnvDependency;
use crate::tree_sitter::SupportedLanguage;

use super::common::anchor_from_node;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract environment variable references from a parsed source file.
///
/// Walks the entire CST recursively, matching language-specific patterns
/// at each node. Returns an empty vector for languages without known
/// env access patterns.
pub fn extract_env_dependencies(
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
    file_path: &Path,
) -> Vec<EnvDependency> {
    let mut deps = Vec::new();
    walk_node(tree.root_node(), source, language, file_path, &mut deps);
    deps
}

// ---------------------------------------------------------------------------
// Recursive CST walker
// ---------------------------------------------------------------------------

fn walk_node(
    node: Node,
    source: &str,
    language: SupportedLanguage,
    file_path: &Path,
    deps: &mut Vec<EnvDependency>,
) {
    // Try to match the current node against language patterns
    if let Some(dep) = try_match(node, source, language, file_path) {
        deps.push(dep);
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            walk_node(child, source, language, file_path, deps);
        }
    }
}

// ---------------------------------------------------------------------------
// Language-specific matching
// ---------------------------------------------------------------------------

fn try_match(
    node: Node,
    source: &str,
    language: SupportedLanguage,
    file_path: &Path,
) -> Option<EnvDependency> {
    match language {
        SupportedLanguage::TypeScript
        | SupportedLanguage::Tsx
        | SupportedLanguage::JavaScript
        | SupportedLanguage::Jsx => try_match_js(node, source, file_path),
        SupportedLanguage::Python => try_match_python(node, source, file_path),
        SupportedLanguage::Go => try_match_go(node, source, file_path),
        SupportedLanguage::Java | SupportedLanguage::Kotlin => {
            try_match_java(node, source, file_path)
        }
        SupportedLanguage::CSharp => try_match_csharp(node, source, file_path),
        SupportedLanguage::Php => try_match_php(node, source, file_path),
        SupportedLanguage::Ruby => try_match_ruby(node, source, file_path),
        SupportedLanguage::Rust => try_match_rust(node, source, file_path),
        _ => None,
    }
}

/// JS/TS: `process.env.VAR_NAME` or `import.meta.env.VAR_NAME`
///
/// CST shape: `member_expression` → `object: member_expression("process.env")` + `property: property_identifier("VAR_NAME")`
/// Dynamic: `process.env[varName]` → `subscript_expression` → `"<dynamic>"`
fn try_match_js(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let kind = node.kind();

    if kind == "member_expression" || kind == "subscript_expression" {
        // Check for process.env.X or import.meta.env.X pattern
        if text.starts_with("process.env.") || text.starts_with("import.meta.env.") {
            let var_name = text.rsplit('.').next().unwrap_or("<dynamic>").to_string();
            // Avoid matching the parent `process.env` part itself
            if var_name != "env" {
                return Some(EnvDependency {
                    var_name,
                    anchor: anchor_from_node(&node, file_path),
                });
            }
        }
        // Dynamic access: process.env[something]
        if kind == "subscript_expression"
            && (text.starts_with("process.env[") || text.starts_with("import.meta.env["))
        {
            return Some(EnvDependency {
                var_name: "<dynamic>".to_string(),
                anchor: anchor_from_node(&node, file_path),
            });
        }
    }

    None
}

/// Python: `os.getenv("VAR")`, `os.environ["VAR"]`, `os.environ.get("VAR")`
fn try_match_python(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let kind = node.kind();

    if kind == "call" {
        // os.getenv("VAR") or os.environ.get("VAR")
        if text.starts_with("os.getenv(") || text.starts_with("os.environ.get(") {
            let var_name = extract_first_string_arg(node, source)?;
            return Some(EnvDependency {
                var_name,
                anchor: anchor_from_node(&node, file_path),
            });
        }
    }

    if kind == "subscript" {
        // os.environ["VAR"]
        if text.starts_with("os.environ[") {
            let var_name = extract_subscript_string(node, source)?;
            return Some(EnvDependency {
                var_name,
                anchor: anchor_from_node(&node, file_path),
            });
        }
    }

    None
}

/// Go: `os.Getenv("VAR")`
fn try_match_go(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;

    if node.kind() == "call_expression" && text.starts_with("os.Getenv(") {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    None
}

/// Java: `System.getenv("VAR")`
fn try_match_java(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;

    if node.kind() == "method_invocation" && text.starts_with("System.getenv(") {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    // @Value("${VAR}") annotation
    if (node.kind() == "annotation" || node.kind() == "marker_annotation")
        && text.starts_with("@Value(")
    {
        if let Some(start) = text.find("${") {
            if let Some(end) = text[start..].find('}') {
                let var_name = text[start + 2..start + end].to_string();
                return Some(EnvDependency {
                    var_name,
                    anchor: anchor_from_node(&node, file_path),
                });
            }
        }
    }

    None
}

/// C#: `Environment.GetEnvironmentVariable("VAR")`
fn try_match_csharp(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;

    if node.kind() == "invocation_expression"
        && text.starts_with("Environment.GetEnvironmentVariable(")
    {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    None
}

/// PHP: `getenv("VAR")`, `env("VAR")` (Laravel), `$_ENV["VAR"]`
fn try_match_php(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let kind = node.kind();

    if kind == "function_call_expression"
        && (text.starts_with("getenv(") || text.starts_with("env("))
    {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    if kind == "subscript_expression" && text.starts_with("$_ENV[") {
        let var_name = extract_subscript_string(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    None
}

/// Ruby: `ENV["VAR"]`, `ENV.fetch("VAR")`
fn try_match_ruby(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let kind = node.kind();

    if kind == "element_reference" && text.starts_with("ENV[") {
        let var_name = extract_subscript_string(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    if (kind == "call" || kind == "method_call") && text.starts_with("ENV.fetch(") {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    None
}

/// Rust: `std::env::var("VAR")`, `env::var("VAR")`, `env!("VAR")`
fn try_match_rust(node: Node, source: &str, file_path: &Path) -> Option<EnvDependency> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let kind = node.kind();

    if kind == "call_expression"
        && (text.starts_with("std::env::var(")
            || text.starts_with("env::var(")
            || text.starts_with("std::env::var_os(")
            || text.starts_with("env::var_os("))
    {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    // env!("VAR") macro invocation
    if kind == "macro_invocation" && text.starts_with("env!(") {
        let var_name = extract_first_string_arg(node, source)?;
        return Some(EnvDependency {
            var_name,
            anchor: anchor_from_node(&node, file_path),
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the first string literal argument from a call expression.
///
/// Walks child nodes looking for an argument_list/arguments containing a
/// string/string_literal node, then strips quotes.
fn extract_first_string_arg(node: Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        let child = node.child(i as u32)?;
        let kind = child.kind();
        if kind == "arguments"
            || kind == "argument_list"
            || kind == "token_tree"
            || kind == "actual_parameters"
        {
            // Walk inside the arguments to find the first string
            for j in 0..child.child_count() {
                if let Some(arg) = child.child(j as u32) {
                    if let Some(name) = extract_string_value(arg, source) {
                        return Some(name);
                    }
                    // Some grammars nest in expression_statement or keyword_argument
                    for k in 0..arg.child_count() {
                        if let Some(inner) = arg.child(k as u32) {
                            if let Some(name) = extract_string_value(inner, source) {
                                return Some(name);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract a string value from a subscript/index access: `obj["KEY"]`.
fn extract_subscript_string(node: Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if let Some(name) = extract_string_value(child, source) {
                return Some(name);
            }
        }
    }
    None
}

/// Extract the text value from a string literal node, stripping quotes.
fn extract_string_value(node: Node, source: &str) -> Option<String> {
    let kind = node.kind();
    if kind == "string"
        || kind == "string_literal"
        || kind == "interpreted_string_literal"
        || kind == "string_value"
        || kind == "encapsed_string"
    {
        let text = node.utf8_text(source.as_bytes()).ok()?;
        let stripped = text.trim_matches('"').trim_matches('\'').trim_matches('`');
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    // Some grammars wrap strings — check child string_content
    if kind == "string" || kind == "string_literal" {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "string_content" || child.kind() == "string_fragment" {
                    if let Ok(text) = child.utf8_text(source.as_bytes()) {
                        if !text.is_empty() {
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::tree_sitter;

    fn env_deps_for(source: &str, lang: SupportedLanguage, filename: &str) -> Vec<EnvDependency> {
        let path = PathBuf::from(filename);
        let parsed = crate::tree_sitter::parse_source(&path, source, lang, None).unwrap();
        extract_env_dependencies(source, &parsed.tree, lang, &path)
    }

    // --- TypeScript/JS ---

    #[test]
    fn ts_process_env_detected() {
        let deps = env_deps_for(
            "const port = process.env.PORT;\n",
            SupportedLanguage::TypeScript,
            "config.ts",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    #[test]
    fn ts_multiple_env_vars() {
        let deps = env_deps_for(
            "const a = process.env.DB_HOST;\nconst b = process.env.DB_PORT;\n",
            SupportedLanguage::TypeScript,
            "config.ts",
        );
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.var_name == "DB_HOST"));
        assert!(deps.iter().any(|d| d.var_name == "DB_PORT"));
    }

    #[test]
    fn ts_dynamic_env_access() {
        let deps = env_deps_for(
            "const val = process.env[key];\n",
            SupportedLanguage::TypeScript,
            "config.ts",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "<dynamic>");
    }

    // --- Python ---

    #[test]
    fn python_os_getenv_detected() {
        let deps = env_deps_for(
            "import os\nport = os.getenv(\"PORT\")\n",
            SupportedLanguage::Python,
            "config.py",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    #[test]
    fn python_os_environ_subscript_detected() {
        let deps = env_deps_for(
            "import os\ndb_url = os.environ[\"DATABASE_URL\"]\n",
            SupportedLanguage::Python,
            "config.py",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "DATABASE_URL");
    }

    #[test]
    fn python_os_environ_get_detected() {
        let deps = env_deps_for(
            "import os\napi_key = os.environ.get(\"API_KEY\")\n",
            SupportedLanguage::Python,
            "config.py",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "API_KEY");
    }

    // --- Go ---

    #[test]
    fn go_os_getenv_detected() {
        let deps = env_deps_for(
            "package main\nimport \"os\"\nfunc main() {\n    port := os.Getenv(\"PORT\")\n    _ = port\n}\n",
            SupportedLanguage::Go,
            "main.go",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    // --- Java ---

    #[test]
    fn java_system_getenv_detected() {
        let deps = env_deps_for(
            "public class Config {\n    String port = System.getenv(\"PORT\");\n}\n",
            SupportedLanguage::Java,
            "Config.java",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    // --- C# ---

    #[test]
    fn csharp_environment_get_detected() {
        let deps = env_deps_for(
            "public class Config {\n    string port = Environment.GetEnvironmentVariable(\"PORT\");\n}\n",
            SupportedLanguage::CSharp,
            "Config.cs",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    // --- PHP ---

    #[test]
    fn php_getenv_detected() {
        let deps = env_deps_for(
            "<?php\n$port = getenv('PORT');\n?>",
            SupportedLanguage::Php,
            "config.php",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    #[test]
    fn php_env_laravel_detected() {
        let deps = env_deps_for(
            "<?php\n$port = env('APP_PORT');\n?>",
            SupportedLanguage::Php,
            "config.php",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "APP_PORT");
    }

    // --- Ruby ---

    #[test]
    fn ruby_env_subscript_detected() {
        let deps = env_deps_for(
            "port = ENV[\"PORT\"]\n",
            SupportedLanguage::Ruby,
            "config.rb",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    #[test]
    fn ruby_env_fetch_detected() {
        let deps = env_deps_for(
            "port = ENV.fetch(\"PORT\")\n",
            SupportedLanguage::Ruby,
            "config.rb",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    // --- Rust ---

    #[test]
    fn rust_env_var_detected() {
        let deps = env_deps_for(
            "fn main() {\n    let port = std::env::var(\"PORT\").unwrap();\n}\n",
            SupportedLanguage::Rust,
            "main.rs",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "PORT");
    }

    #[test]
    fn rust_env_macro_detected() {
        let deps = env_deps_for(
            "const VERSION: &str = env!(\"CARGO_PKG_VERSION\");\n",
            SupportedLanguage::Rust,
            "main.rs",
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].var_name, "CARGO_PKG_VERSION");
    }

    // --- Edge cases ---

    #[test]
    fn no_env_vars_returns_empty() {
        let deps = env_deps_for(
            "function hello() { return 'world'; }\n",
            SupportedLanguage::TypeScript,
            "hello.ts",
        );
        assert!(deps.is_empty());
    }

    #[test]
    fn unsupported_language_returns_empty() {
        let deps = env_deps_for("let x = 1;", SupportedLanguage::Swift, "main.swift");
        assert!(deps.is_empty());
    }
}
