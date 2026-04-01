//! Language-aware extraction dispatch.
//!
//! Routes each file to the appropriate extractor based on its language family.
//! Languages in the same family share enough CST node structure to reuse
//! a single extractor (e.g., JS/TS/JSX/TSX all use `call_expression`).
//!
//! Dedicated extractors exist for the 7 most common backend framework families,
//! covering ~78% of backend framework market share. Languages without a dedicated
//! extractor fall through to the generic extractor for log sink detection.

pub mod call_graph;
pub mod common;
pub mod csharp;
pub mod data_models;
pub mod env_detection;
pub mod generic;
pub mod go;
pub mod java;
pub mod language_behavior;
pub mod php;
pub mod python;
pub mod ruby;
pub mod symbols;
pub mod type_hierarchy;
pub mod typescript;

use std::path::Path;

use tree_sitter::Tree;

use crate::tree_sitter::{LanguageFamily, SupportedLanguage};

use crate::types::{estimate_tokens, FileExtraction, FileRole};

/// Extract semantic information from a parsed source file.
///
/// Dispatches to the appropriate extractor based on the language family:
/// - `JavaScriptLike` (TS, TSX, JS, JSX) → Express/Koa/Hapi route + auth + HTTP call + log + import
/// - `Python`                             → FastAPI/Flask/Django route + auth decorator + HTTP call + log
/// - `JvmLike` (Java, Kotlin, Scala)      → Spring Boot annotation route + auth + HTTP call + log
/// - `CSharp`                             → ASP.NET Core attribute route + auth + HTTP call + log
/// - `Go`                                 → Gin/Echo/net-http route + middleware auth + HTTP call + log
/// - `Php`                                → Laravel Route:: route + middleware auth + HTTP call + log
/// - `Ruby`                               → Rails route DSL + before_action auth + HTTP call + log
/// - All others (Rust, Swift, C, C++)     → generic log sink detection with PII scanning
///
/// After language-specific extraction, enriches the result with:
/// - File role classification via `FileRole::from_path()`
/// - Token estimation via `estimate_tokens()`
pub fn extract(
    file_path: &Path,
    source: &str,
    tree: &Tree,
    language: SupportedLanguage,
) -> FileExtraction {
    let mut extraction = match language.family() {
        LanguageFamily::JavaScriptLike => typescript::extract(file_path, source, tree, language),
        LanguageFamily::Python => python::extract(file_path, source, tree, language),
        LanguageFamily::JvmLike => java::extract(file_path, source, tree, language),
        LanguageFamily::CSharp => csharp::extract(file_path, source, tree, language),
        LanguageFamily::Go => go::extract(file_path, source, tree, language),
        LanguageFamily::Php => php::extract(file_path, source, tree, language),
        LanguageFamily::Ruby => ruby::extract(file_path, source, tree, language),
        _ => generic::extract(file_path, source, tree, language),
    };

    // Extract code-level symbols (classes, functions, methods, etc.)
    extraction.symbols = symbols::extract_symbols(source, tree, language, file_path);

    // Extract call graph references
    extraction.references =
        call_graph::extract_call_sites(source, tree, language, file_path, &extraction.symbols);

    // Extract type hierarchy (extends/implements) references
    extraction
        .references
        .extend(type_hierarchy::extract_type_hierarchy(
            source, tree, language, file_path,
        ));

    // Extract data models (classes, structs, interfaces with fields)
    extraction.data_models = data_models::extract_data_models(source, tree, language, file_path);

    // Extract environment variable references
    extraction.env_dependencies =
        env_detection::extract_env_dependencies(source, tree, language, file_path);

    // Enrich with file role and token estimation
    extraction.file_role = FileRole::from_path(file_path);
    extraction.estimated_tokens = estimate_tokens(source.len() as u64);

    extraction
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use crate::tree_sitter;

    /// Helper: parse and extract a source file.
    fn parse_and_extract(source: &str, lang: SupportedLanguage, filename: &str) -> FileExtraction {
        let path = PathBuf::from(filename);
        let parsed = tree_sitter::parse_source(&path, source, lang, None).unwrap();
        extract(&path, source, &parsed.tree, lang)
    }

    #[test]
    fn python_dispatch_extracts_routes() {
        let ext = parse_and_extract(
            r#"
from fastapi import FastAPI
app = FastAPI()

@app.get("/api/users")
def list_users():
    return []
"#,
            SupportedLanguage::Python,
            "app.py",
        );
        assert!(
            !ext.interfaces.is_empty(),
            "Python dispatch should extract FastAPI routes"
        );
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
    }

    #[test]
    fn java_dispatch_extracts_routes() {
        let ext = parse_and_extract(
            r#"
public class UserController {
    @GetMapping("/api/users")
    public List<User> list() { return List.of(); }
}
"#,
            SupportedLanguage::Java,
            "UserController.java",
        );
        assert!(
            !ext.interfaces.is_empty(),
            "Java dispatch should extract Spring routes"
        );
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
    }

    #[test]
    fn csharp_dispatch_extracts_routes() {
        let ext = parse_and_extract(
            r#"
public class UsersController : ControllerBase {
    [HttpGet("api/users")]
    public IActionResult List() { return Ok(); }
}
"#,
            SupportedLanguage::CSharp,
            "UsersController.cs",
        );
        assert!(
            !ext.interfaces.is_empty(),
            "C# dispatch should extract ASP.NET routes"
        );
    }

    #[test]
    fn go_dispatch_extracts_routes() {
        let ext = parse_and_extract(
            r#"
package main
func main() {
    r := gin.Default()
    r.GET("/api/users", listUsers)
}
"#,
            SupportedLanguage::Go,
            "main.go",
        );
        assert!(
            !ext.interfaces.is_empty(),
            "Go dispatch should extract Gin routes"
        );
    }

    #[test]
    fn php_dispatch_extracts_routes() {
        let ext = parse_and_extract(
            r#"<?php
Route::get('/api/users', [UserController::class, 'index']);
?>"#,
            SupportedLanguage::Php,
            "routes.php",
        );
        assert!(
            !ext.interfaces.is_empty(),
            "PHP dispatch should extract Laravel routes"
        );
    }

    #[test]
    fn ruby_dispatch_extracts_routes() {
        let ext = parse_and_extract(
            r#"
get '/api/users', to: 'users#index'
"#,
            SupportedLanguage::Ruby,
            "routes.rb",
        );
        assert!(
            !ext.interfaces.is_empty(),
            "Ruby dispatch should extract Rails routes"
        );
    }

    #[test]
    fn rust_falls_through_to_generic() {
        let ext = parse_and_extract(
            r#"
fn main() {
    log.info("Starting server");
}
"#,
            SupportedLanguage::Rust,
            "main.rs",
        );
        // Rust uses generic extractor — no routes, but log sinks detected
        assert!(ext.interfaces.is_empty());
        // log.info should be detected as a sink
        assert!(ext.sinks.len() >= 1);
    }
}
