//! Sibling test body of `language_behavior.rs` (T2.2 of god-files-2026-07-23-plan.md).


#![cfg(test)]

#![allow(unused_imports)]

use super::*;
use super::trait_def::*;
use super::dispatch::*;
use super::typescript::*;
use super::python::*;
use super::java::*;
use super::csharp::*;
use super::go::*;
use super::php::*;
use super::ruby::*;
use super::rust::*;
use super::generic::*;
use crate::tree_sitter::SupportedLanguage;
use crate::types::Visibility;

    // =======================================================================
    // module_separator
    // =======================================================================

    #[test]
    fn typescript_module_separator_is_dot() {
        assert_eq!(TypeScriptBehavior.module_separator(), ".");
    }

    #[test]
    fn python_module_separator_is_dot() {
        assert_eq!(PythonBehavior.module_separator(), ".");
    }

    #[test]
    fn java_module_separator_is_dot() {
        assert_eq!(JavaBehavior.module_separator(), ".");
    }

    #[test]
    fn csharp_module_separator_is_dot() {
        assert_eq!(CSharpBehavior.module_separator(), ".");
    }

    #[test]
    fn go_module_separator_is_dot() {
        assert_eq!(GoBehavior.module_separator(), ".");
    }

    #[test]
    fn php_module_separator_is_backslash() {
        assert_eq!(PhpBehavior.module_separator(), "\\");
    }

    #[test]
    fn ruby_module_separator_is_double_colon() {
        assert_eq!(RubyBehavior.module_separator(), "::");
    }

    #[test]
    fn rust_module_separator_is_double_colon() {
        assert_eq!(RustBehavior.module_separator(), "::");
    }

    #[test]
    fn generic_module_separator_is_dot() {
        assert_eq!(GenericBehavior.module_separator(), ".");
    }

    // =======================================================================
    // source_roots
    // =======================================================================

    #[test]
    fn typescript_source_roots() {
        let roots = TypeScriptBehavior.source_roots();
        assert!(roots.contains(&"src"));
        assert!(roots.contains(&"lib"));
        assert!(roots.contains(&"app"));
    }

    #[test]
    fn python_source_roots() {
        let roots = PythonBehavior.source_roots();
        assert!(roots.contains(&"src"));
        assert!(roots.contains(&"app"));
    }

    #[test]
    fn java_source_roots() {
        let roots = JavaBehavior.source_roots();
        assert!(roots.contains(&"src/main/java"));
        assert!(roots.contains(&"src"));
    }

    #[test]
    fn csharp_source_roots() {
        let roots = CSharpBehavior.source_roots();
        assert!(roots.contains(&"src"));
        assert!(roots.contains(&"Controllers"));
        assert!(roots.contains(&"Services"));
    }

    #[test]
    fn go_source_roots() {
        let roots = GoBehavior.source_roots();
        assert!(roots.contains(&"cmd"));
        assert!(roots.contains(&"internal"));
        assert!(roots.contains(&"pkg"));
    }

    #[test]
    fn php_source_roots() {
        let roots = PhpBehavior.source_roots();
        assert!(roots.contains(&"src"));
        assert!(roots.contains(&"app"));
    }

    #[test]
    fn ruby_source_roots() {
        let roots = RubyBehavior.source_roots();
        assert!(roots.contains(&"app"));
        assert!(roots.contains(&"lib"));
    }

    #[test]
    fn rust_source_roots() {
        let roots = RustBehavior.source_roots();
        assert!(roots.contains(&"src"));
        assert_eq!(roots.len(), 1);
    }

    #[test]
    fn generic_source_roots() {
        let roots = GenericBehavior.source_roots();
        assert!(roots.contains(&"src"));
    }

    // =======================================================================
    // call_node_kinds
    // =======================================================================

    #[test]
    fn typescript_call_node_kinds() {
        let kinds = TypeScriptBehavior.call_node_kinds();
        assert!(kinds.contains(&"call_expression"));
    }

    #[test]
    fn python_call_node_kinds() {
        let kinds = PythonBehavior.call_node_kinds();
        assert!(kinds.contains(&"call"));
    }

    #[test]
    fn java_call_node_kinds() {
        let kinds = JavaBehavior.call_node_kinds();
        assert!(kinds.contains(&"method_invocation"));
    }

    #[test]
    fn csharp_call_node_kinds() {
        let kinds = CSharpBehavior.call_node_kinds();
        assert!(kinds.contains(&"invocation_expression"));
    }

    #[test]
    fn go_call_node_kinds() {
        let kinds = GoBehavior.call_node_kinds();
        assert!(kinds.contains(&"call_expression"));
    }

    #[test]
    fn php_call_node_kinds() {
        let kinds = PhpBehavior.call_node_kinds();
        assert!(kinds.contains(&"member_call_expression"));
        assert!(kinds.contains(&"function_call_expression"));
        assert!(kinds.contains(&"scoped_call_expression"));
    }

    #[test]
    fn ruby_call_node_kinds() {
        let kinds = RubyBehavior.call_node_kinds();
        assert!(kinds.contains(&"call"));
        assert!(kinds.contains(&"method_call"));
    }

    #[test]
    fn rust_call_node_kinds() {
        let kinds = RustBehavior.call_node_kinds();
        assert!(kinds.contains(&"call_expression"));
    }

    #[test]
    fn generic_call_node_kinds() {
        let kinds = GenericBehavior.call_node_kinds();
        assert!(kinds.contains(&"call_expression"));
    }

    // =======================================================================
    // behavior_for factory
    // =======================================================================

    #[test]
    fn factory_maps_typescript_family() {
        let b = behavior_for(SupportedLanguage::TypeScript);
        assert_eq!(b.module_separator(), ".");
        assert!(b.source_roots().contains(&"lib"));

        // All JS-like languages should get the same behavior
        let tsx = behavior_for(SupportedLanguage::Tsx);
        assert_eq!(tsx.module_separator(), ".");
        assert!(tsx.source_roots().contains(&"lib"));

        let js = behavior_for(SupportedLanguage::JavaScript);
        assert_eq!(js.module_separator(), ".");

        let jsx = behavior_for(SupportedLanguage::Jsx);
        assert_eq!(jsx.module_separator(), ".");
    }

    #[test]
    fn factory_maps_python() {
        let b = behavior_for(SupportedLanguage::Python);
        assert_eq!(b.module_separator(), ".");
        assert!(b.call_node_kinds().contains(&"call"));
    }

    #[test]
    fn factory_maps_java_and_kotlin() {
        let java = behavior_for(SupportedLanguage::Java);
        assert!(java.call_node_kinds().contains(&"method_invocation"));

        let kotlin = behavior_for(SupportedLanguage::Kotlin);
        assert!(kotlin.call_node_kinds().contains(&"method_invocation"));
    }

    #[test]
    fn factory_maps_csharp() {
        let b = behavior_for(SupportedLanguage::CSharp);
        assert!(b.call_node_kinds().contains(&"invocation_expression"));
    }

    #[test]
    fn factory_maps_go() {
        let b = behavior_for(SupportedLanguage::Go);
        assert!(b.source_roots().contains(&"cmd"));
    }

    #[test]
    fn factory_maps_php() {
        let b = behavior_for(SupportedLanguage::Php);
        assert_eq!(b.module_separator(), "\\");
    }

    #[test]
    fn factory_maps_ruby() {
        let b = behavior_for(SupportedLanguage::Ruby);
        assert_eq!(b.module_separator(), "::");
    }

    #[test]
    fn factory_maps_rust() {
        let b = behavior_for(SupportedLanguage::Rust);
        assert_eq!(b.module_separator(), "::");
    }

    #[test]
    fn factory_maps_generic_languages() {
        for lang in [
            SupportedLanguage::C,
            SupportedLanguage::Cpp,
            SupportedLanguage::Swift,
        ] {
            let b = behavior_for(lang);
            assert_eq!(b.module_separator(), ".");
            assert!(b.call_node_kinds().contains(&"call_expression"));
        }
    }

    #[test]
    fn factory_maps_scala_to_java_behavior() {
        let b = behavior_for(SupportedLanguage::Scala);
        assert!(b.call_node_kinds().contains(&"method_invocation"));
        assert!(b.source_roots().contains(&"src/main/java"));
    }

    // =======================================================================
    // signature_body_opener
    // =======================================================================

    #[test]
    fn python_body_opener_is_colon() {
        assert_eq!(PythonBehavior.signature_body_opener(), Some(':'));
    }

    #[test]
    fn ruby_body_opener_is_none() {
        assert_eq!(RubyBehavior.signature_body_opener(), None);
    }

    #[test]
    fn c_family_body_opener_is_brace() {
        assert_eq!(TypeScriptBehavior.signature_body_opener(), Some('{'));
        assert_eq!(JavaBehavior.signature_body_opener(), Some('{'));
        assert_eq!(CSharpBehavior.signature_body_opener(), Some('{'));
        assert_eq!(GoBehavior.signature_body_opener(), Some('{'));
        assert_eq!(RustBehavior.signature_body_opener(), Some('{'));
        assert_eq!(GenericBehavior.signature_body_opener(), Some('{'));
    }

    // =======================================================================
    // truncate_at_char helper
    // =======================================================================

    #[test]
    fn truncate_at_char_finds_first_occurrence() {
        assert_eq!(
            truncate_at_char("fn main() {", '{'),
            Some("fn main()".to_string())
        );
    }

    #[test]
    fn truncate_at_char_returns_none_when_absent() {
        assert_eq!(truncate_at_char("no opener here", '{'), None);
    }

    #[test]
    fn truncate_at_char_trims_whitespace() {
        assert_eq!(
            truncate_at_char("def foo()  :", ':'),
            Some("def foo()".to_string())
        );
    }

    // =======================================================================
    // is_stdlib_module
    // =======================================================================

    #[test]
    fn python_recognizes_stdlib_modules() {
        assert!(PythonBehavior.is_stdlib_module("os"));
        assert!(PythonBehavior.is_stdlib_module("sys"));
        assert!(PythonBehavior.is_stdlib_module("json"));
        assert!(PythonBehavior.is_stdlib_module("collections"));
        assert!(PythonBehavior.is_stdlib_module("asyncio"));
        assert!(PythonBehavior.is_stdlib_module("typing"));
        assert!(PythonBehavior.is_stdlib_module("pathlib"));
    }

    #[test]
    fn python_rejects_third_party_modules() {
        assert!(!PythonBehavior.is_stdlib_module("fastapi"));
        assert!(!PythonBehavior.is_stdlib_module("torch"));
        assert!(!PythonBehavior.is_stdlib_module("numpy"));
        assert!(!PythonBehavior.is_stdlib_module("requests"));
        assert!(!PythonBehavior.is_stdlib_module("pydantic"));
        assert!(!PythonBehavior.is_stdlib_module("django"));
    }

    #[test]
    fn non_python_languages_return_false_for_stdlib() {
        assert!(!TypeScriptBehavior.is_stdlib_module("os"));
        assert!(!JavaBehavior.is_stdlib_module("java"));
        assert!(!GoBehavior.is_stdlib_module("fmt"));
        assert!(!GenericBehavior.is_stdlib_module("std"));
    }

    // =======================================================================
    // is_builtin_symbol
    // =======================================================================

    #[test]
    fn python_recognizes_builtin_types_and_functions() {
        assert!(PythonBehavior.is_builtin_symbol("int"));
        assert!(PythonBehavior.is_builtin_symbol("str"));
        assert!(PythonBehavior.is_builtin_symbol("list"));
        assert!(PythonBehavior.is_builtin_symbol("dict"));
        assert!(PythonBehavior.is_builtin_symbol("print"));
        assert!(PythonBehavior.is_builtin_symbol("len"));
        assert!(PythonBehavior.is_builtin_symbol("range"));
        assert!(PythonBehavior.is_builtin_symbol("isinstance"));
        assert!(PythonBehavior.is_builtin_symbol("super"));
        assert!(PythonBehavior.is_builtin_symbol("property"));
        assert!(PythonBehavior.is_builtin_symbol("staticmethod"));
        assert!(PythonBehavior.is_builtin_symbol("classmethod"));
    }

    #[test]
    fn python_recognizes_builtin_exceptions() {
        assert!(PythonBehavior.is_builtin_symbol("Exception"));
        assert!(PythonBehavior.is_builtin_symbol("ValueError"));
        assert!(PythonBehavior.is_builtin_symbol("TypeError"));
        assert!(PythonBehavior.is_builtin_symbol("KeyError"));
        assert!(PythonBehavior.is_builtin_symbol("AttributeError"));
        assert!(PythonBehavior.is_builtin_symbol("RuntimeError"));
        assert!(PythonBehavior.is_builtin_symbol("NotImplementedError"));
        assert!(PythonBehavior.is_builtin_symbol("StopIteration"));
    }

    #[test]
    fn python_recognizes_builtin_constants() {
        assert!(PythonBehavior.is_builtin_symbol("True"));
        assert!(PythonBehavior.is_builtin_symbol("False"));
        assert!(PythonBehavior.is_builtin_symbol("None"));
    }

    #[test]
    fn python_rejects_non_builtin_symbols() {
        assert!(!PythonBehavior.is_builtin_symbol("torch"));
        assert!(!PythonBehavior.is_builtin_symbol("numpy"));
        assert!(!PythonBehavior.is_builtin_symbol("MyClass"));
        assert!(!PythonBehavior.is_builtin_symbol("custom_func"));
    }

    #[test]
    fn typescript_recognizes_builtin_globals() {
        assert!(TypeScriptBehavior.is_builtin_symbol("console"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Promise"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Array"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Object"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Map"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Set"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Error"));
        assert!(TypeScriptBehavior.is_builtin_symbol("JSON"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Math"));
        assert!(TypeScriptBehavior.is_builtin_symbol("Date"));
        assert!(TypeScriptBehavior.is_builtin_symbol("setTimeout"));
        assert!(TypeScriptBehavior.is_builtin_symbol("fetch"));
    }

    #[test]
    fn typescript_rejects_non_builtin_symbols() {
        assert!(!TypeScriptBehavior.is_builtin_symbol("express"));
        assert!(!TypeScriptBehavior.is_builtin_symbol("MyComponent"));
        assert!(!TypeScriptBehavior.is_builtin_symbol("lodash"));
    }

    #[test]
    fn non_python_ts_languages_return_false_for_builtins() {
        assert!(!JavaBehavior.is_builtin_symbol("int"));
        assert!(!GoBehavior.is_builtin_symbol("fmt"));
        assert!(!GenericBehavior.is_builtin_symbol("print"));
    }
