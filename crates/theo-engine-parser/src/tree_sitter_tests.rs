//! Sibling test body of `tree_sitter.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `tree_sitter.rs` via `#[path = "tree_sitter_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;
    use std::path::PathBuf;

    // --- Language detection tests ---

    #[test]
    fn detects_typescript_extensions() {
        assert_eq!(
            detect_language(Path::new("app.ts")),
            Some(SupportedLanguage::TypeScript)
        );
        assert_eq!(
            detect_language(Path::new("mod.mts")),
            Some(SupportedLanguage::TypeScript)
        );
        assert_eq!(
            detect_language(Path::new("mod.cts")),
            Some(SupportedLanguage::TypeScript)
        );
        assert_eq!(
            detect_language(Path::new("App.tsx")),
            Some(SupportedLanguage::Tsx)
        );
    }

    #[test]
    fn detects_javascript_extensions() {
        assert_eq!(
            detect_language(Path::new("app.js")),
            Some(SupportedLanguage::JavaScript)
        );
        assert_eq!(
            detect_language(Path::new("mod.mjs")),
            Some(SupportedLanguage::JavaScript)
        );
        assert_eq!(
            detect_language(Path::new("mod.cjs")),
            Some(SupportedLanguage::JavaScript)
        );
        assert_eq!(
            detect_language(Path::new("App.jsx")),
            Some(SupportedLanguage::Jsx)
        );
    }

    #[test]
    fn detects_all_supported_languages() {
        let cases = vec![
            ("main.py", SupportedLanguage::Python),
            ("App.java", SupportedLanguage::Java),
            ("Program.cs", SupportedLanguage::CSharp),
            ("main.go", SupportedLanguage::Go),
            ("main.rs", SupportedLanguage::Rust),
            ("index.php", SupportedLanguage::Php),
            ("app.rb", SupportedLanguage::Ruby),
            ("Main.kt", SupportedLanguage::Kotlin),
            ("build.kts", SupportedLanguage::Kotlin),
            ("App.swift", SupportedLanguage::Swift),
            ("main.c", SupportedLanguage::C),
            ("utils.h", SupportedLanguage::C),
            ("main.cpp", SupportedLanguage::Cpp),
            ("util.cc", SupportedLanguage::Cpp),
            ("lib.hpp", SupportedLanguage::Cpp),
            ("Main.scala", SupportedLanguage::Scala),
        ];

        for (file, expected) in cases {
            assert_eq!(
                detect_language(Path::new(file)),
                Some(expected),
                "failed for {file}"
            );
        }
    }

    #[test]
    fn returns_none_for_unsupported() {
        assert_eq!(detect_language(Path::new("readme.md")), None);
        assert_eq!(detect_language(Path::new("data.json")), None);
        assert_eq!(detect_language(Path::new("style.css")), None);
        assert_eq!(detect_language(Path::new("noext")), None);
    }

    // --- Parse tests per language ---

    #[test]
    fn parses_typescript() {
        let source = "const x: number = 42;";
        let parsed = parse_source(
            &PathBuf::from("t.ts"),
            source,
            SupportedLanguage::TypeScript,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_tsx() {
        let source = "const App = () => <div>Hi</div>;";
        let parsed = parse_source(
            &PathBuf::from("t.tsx"),
            source,
            SupportedLanguage::Tsx,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_javascript() {
        let source = "const x = 42; function greet() { return 'hello'; }";
        let parsed = parse_source(
            &PathBuf::from("t.js"),
            source,
            SupportedLanguage::JavaScript,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_python() {
        let source = r#"
def hello(name: str) -> str:
    return f"Hello, {name}"

class UserService:
    def get_user(self, user_id: int):
        pass
"#;
        let parsed = parse_source(
            &PathBuf::from("t.py"),
            source,
            SupportedLanguage::Python,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_java() {
        let source = r#"
public class HelloWorld {
    public static void main(String[] args) {
        System.out.println("Hello");
    }
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.java"),
            source,
            SupportedLanguage::Java,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_csharp() {
        let source = r#"
using System;
namespace App {
    class Program {
        static void Main(string[] args) {
            Console.WriteLine("Hello");
        }
    }
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.cs"),
            source,
            SupportedLanguage::CSharp,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_go() {
        let source = r#"
package main

import "fmt"

func main() {
    fmt.Println("Hello")
}
"#;
        let parsed =
            parse_source(&PathBuf::from("t.go"), source, SupportedLanguage::Go, None).unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_rust() {
        let source = r#"
fn main() {
    let x: i32 = 42;
    println!("value: {x}");
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.rs"),
            source,
            SupportedLanguage::Rust,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_php() {
        let source = r#"<?php
function greet($name) {
    echo "Hello, $name";
}
?>"#;
        let parsed = parse_source(
            &PathBuf::from("t.php"),
            source,
            SupportedLanguage::Php,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_ruby() {
        let source = r#"
class Greeter
  def hello(name)
    puts "Hello, #{name}"
  end
end
"#;
        let parsed = parse_source(
            &PathBuf::from("t.rb"),
            source,
            SupportedLanguage::Ruby,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_kotlin() {
        let source = r#"
fun main() {
    val message = "Hello"
    println(message)
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.kt"),
            source,
            SupportedLanguage::Kotlin,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_swift() {
        let source = r#"
import Foundation
func greet(name: String) -> String {
    return "Hello, \(name)"
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.swift"),
            source,
            SupportedLanguage::Swift,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_c() {
        let source = r#"
#include <stdio.h>
int main() {
    printf("Hello\n");
    return 0;
}
"#;
        let parsed =
            parse_source(&PathBuf::from("t.c"), source, SupportedLanguage::C, None).unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_cpp() {
        let source = r#"
#include <iostream>
int main() {
    std::cout << "Hello" << std::endl;
    return 0;
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.cpp"),
            source,
            SupportedLanguage::Cpp,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn parses_scala() {
        let source = r#"
object Main extends App {
  println("Hello")
}
"#;
        let parsed = parse_source(
            &PathBuf::from("t.scala"),
            source,
            SupportedLanguage::Scala,
            None,
        )
        .unwrap();
        assert!(!parsed.tree.root_node().has_error());
    }

    #[test]
    fn handles_syntax_error_gracefully() {
        let source = "const x = {{{;";
        let parsed = parse_source(
            &PathBuf::from("broken.ts"),
            source,
            SupportedLanguage::TypeScript,
            None,
        )
        .unwrap();
        assert!(parsed.tree.root_node().has_error());
    }

    #[test]
    fn language_family_grouping() {
        assert_eq!(
            SupportedLanguage::TypeScript.family(),
            LanguageFamily::JavaScriptLike
        );
        assert_eq!(
            SupportedLanguage::JavaScript.family(),
            LanguageFamily::JavaScriptLike
        );
        assert_eq!(SupportedLanguage::Java.family(), LanguageFamily::JvmLike);
        assert_eq!(SupportedLanguage::Kotlin.family(), LanguageFamily::JvmLike);
        assert_eq!(SupportedLanguage::Python.family(), LanguageFamily::Python);
        assert_eq!(SupportedLanguage::Go.family(), LanguageFamily::Go);
    }

    #[test]
    fn language_serialization_roundtrip() {
        let lang = SupportedLanguage::CSharp;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"csharp\"");
        let deserialized: SupportedLanguage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, lang);
    }

    // --- InputEdit computation tests ---

    #[test]
    fn compute_input_edit_identical_sources_returns_none() {
        let source = "const x = 42;";
        assert!(compute_input_edit(source, source).is_none());
    }

    #[test]
    fn compute_input_edit_single_line_change() {
        let old = "const x = 42;";
        let new = "const x = 99;";
        let edit = compute_input_edit(old, new).unwrap();

        // The change is at "42" → "99", bytes 10..12
        assert_eq!(edit.start_byte, 10);
        assert_eq!(edit.old_end_byte, 12);
        assert_eq!(edit.new_end_byte, 12);
        assert_eq!(
            edit.start_position,
            tree_sitter::Point { row: 0, column: 10 }
        );
    }

    #[test]
    fn compute_input_edit_insertion() {
        let old = "line1\nline2\n";
        let new = "line1\nnewline\nline2\n";
        let edit = compute_input_edit(old, new).unwrap();

        // Insertion starts at byte 6 (after "line1\n")
        assert_eq!(edit.start_byte, 6);
        assert_eq!(
            edit.start_position,
            tree_sitter::Point { row: 1, column: 0 }
        );
        // old_end_byte and new_end_byte differ by the insertion length
        assert!(edit.new_end_byte > edit.old_end_byte);
    }

    #[test]
    fn compute_input_edit_deletion() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nccc\n";
        let edit = compute_input_edit(old, new).unwrap();

        // "bbb\n" was deleted starting at byte 4
        assert_eq!(edit.start_byte, 4);
        assert!(edit.old_end_byte > edit.new_end_byte);
    }

    #[test]
    fn compute_input_edit_multiline_insertion() {
        let old = "fn main() {\n}\n";
        let new = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        let edit = compute_input_edit(old, new).unwrap();

        assert_eq!(edit.start_byte, 12); // after "fn main() {\n"
        assert_eq!(
            edit.start_position,
            tree_sitter::Point { row: 1, column: 0 }
        );
        // new_end should be on a later row than old_end
        assert!(
            edit.new_end_position.row > edit.old_end_position.row
                || edit.new_end_byte > edit.old_end_byte
        );
    }

    #[test]
    fn incremental_parse_matches_full_parse() {
        let source_v1 = r#"
const app = require('express')();
app.get('/health', (req, res) => res.json({ ok: true }));
"#;
        let source_v2 = r#"
const app = require('express')();
app.get('/health', (req, res) => res.json({ ok: true }));
app.post('/api/users', (req, res) => res.status(201).json({}));
"#;
        let path = PathBuf::from("test.ts");

        // Full parse of v1
        let parsed_v1 =
            parse_source(&path, source_v1, SupportedLanguage::TypeScript, None).unwrap();

        // Incremental parse: edit old tree, then parse with it
        let edit = compute_input_edit(source_v1, source_v2).unwrap();
        let mut old_tree = parsed_v1.tree.clone();
        old_tree.edit(&edit);
        let incremental = parse_source(
            &path,
            source_v2,
            SupportedLanguage::TypeScript,
            Some(&old_tree),
        )
        .unwrap();

        // Full parse of v2 (for comparison)
        let full = parse_source(&path, source_v2, SupportedLanguage::TypeScript, None).unwrap();

        // Both should produce the same S-expression
        assert_eq!(
            incremental.tree.root_node().to_sexp(),
            full.tree.root_node().to_sexp(),
            "incremental parse must match full parse"
        );
    }

    // --- Thread-local cached parser tests ---

    #[test]
    fn cached_parser_produces_same_result_as_fresh_parser() {
        let source = "const x: number = 42;";
        let path = PathBuf::from("cached.ts");

        let fresh = parse_source(&path, source, SupportedLanguage::TypeScript, None).unwrap();
        let cached =
            parse_source_cached(&path, source, SupportedLanguage::TypeScript, None).unwrap();

        assert_eq!(
            fresh.tree.root_node().to_sexp(),
            cached.tree.root_node().to_sexp(),
            "cached parse must produce identical CST to fresh parse"
        );
        assert_eq!(fresh.language, cached.language);
    }

    #[test]
    fn cached_parser_reused_across_same_language_files() {
        let path_a = PathBuf::from("a.ts");
        let path_b = PathBuf::from("b.ts");

        let result_a =
            parse_source_cached(&path_a, "const a = 1;", SupportedLanguage::TypeScript, None)
                .unwrap();
        let result_b =
            parse_source_cached(&path_b, "const b = 2;", SupportedLanguage::TypeScript, None)
                .unwrap();

        assert!(!result_a.tree.root_node().has_error());
        assert!(!result_b.tree.root_node().has_error());
    }

    #[test]
    fn cached_parser_handles_multiple_languages() {
        let ts_path = PathBuf::from("app.ts");
        let py_path = PathBuf::from("app.py");

        let ts = parse_source_cached(
            &ts_path,
            "const x = 1;",
            SupportedLanguage::TypeScript,
            None,
        )
        .unwrap();
        let py = parse_source_cached(&py_path, "x = 1", SupportedLanguage::Python, None).unwrap();

        assert_eq!(ts.language, SupportedLanguage::TypeScript);
        assert_eq!(py.language, SupportedLanguage::Python);
        assert!(!ts.tree.root_node().has_error());
        assert!(!py.tree.root_node().has_error());
    }
