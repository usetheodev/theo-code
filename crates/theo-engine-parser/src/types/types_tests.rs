//! Sibling test body of `types.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of god-files-2026-07-23-plan.md).
//! Included from `types.rs` via `#[path = "types_tests.rs"] mod tests;`.


#![cfg(test)]

    #![allow(unused_imports)]
    use super::*;
    use crate::types::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use crate::tree_sitter::SupportedLanguage;

    #[test]
    fn code_model_round_trip_serialization() {
        let model = CodeModel {
            version: "1.0".into(),
            project_name: "test-project".into(),
            components: vec![Component {
                name: "test-service".into(),
                language: SupportedLanguage::TypeScript,
                interfaces: vec![Interface {
                    method: HttpMethod::Get,
                    path: "/api/health".into(),
                    auth: None,
                    anchor: SourceAnchor::from_line(PathBuf::from("src/index.ts"), 10),
                    parameters: vec![],
                    handler_name: None,
                    request_body_type: None,
                }],
                dependencies: vec![],
                sinks: vec![],
                symbols: vec![],
                imports: vec![],
                references: vec![],
                data_models: vec![],
                module_boundaries: vec![],
                env_dependencies: vec![],
            }],
            stats: CodeModelStats {
                files_analyzed: 1,
                total_interfaces: 1,
                total_dependencies: 0,
                total_sinks: 0,
                total_symbols: 0,
                total_imports: 0,
                total_references: 0,
                total_data_models: 0,
                total_modules: 0,
                resolved_references: 0,
                avg_resolution_confidence: 0.0,
                file_roles: HashMap::new(),
                total_estimated_tokens: 0,
                total_directories: 0,
                total_test_symbols: 0,
                total_env_dependencies: 0,
                resolution_method_distribution: HashMap::new(),
                git_stats: None,
            },
        };

        let json = serde_json::to_string(&model).unwrap();
        let deserialized: CodeModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }

    #[test]
    fn file_extraction_round_trip_serialization() {
        let extraction = FileExtraction {
            file: PathBuf::from("src/server.ts"),
            language: SupportedLanguage::TypeScript,
            interfaces: vec![Interface {
                method: HttpMethod::Post,
                path: "/api/users".into(),
                auth: Some(AuthKind::Middleware("authMiddleware".into())),
                anchor: SourceAnchor::from_line(PathBuf::from("src/server.ts"), 15),
                parameters: vec![],
                handler_name: None,
                request_body_type: None,
            }],
            dependencies: vec![Dependency {
                target: "fetch(\"https://api.example.com\")".into(),
                dependency_type: DependencyType::HttpCall,
                anchor: SourceAnchor::from_line(PathBuf::from("src/server.ts"), 20),
            }],
            sinks: vec![Sink {
                sink_type: SinkType::Log,
                anchor: SourceAnchor::from_line(PathBuf::from("src/server.ts"), 25),
                text: "console.log(user.email)".into(),
                contains_pii: true,
            }],
            imports: vec![ImportInfo {
                source: "express".into(),
                specifiers: vec!["express".into()],
                line: 1,
                aliases: vec![],
            }],
            symbols: vec![],
            references: vec![],
            data_models: vec![],
            env_dependencies: vec![],
            file_role: FileRole::Implementation,
            estimated_tokens: 250,
            content_hash: None,
            git_metadata: None,
        };

        let json = serde_json::to_string(&extraction).unwrap();
        let deserialized: FileExtraction = serde_json::from_str(&json).unwrap();
        assert_eq!(extraction, deserialized);
    }

    #[test]
    fn interface_with_auth_serialization() {
        let iface = Interface {
            method: HttpMethod::Delete,
            path: "/api/users/:id".into(),
            auth: Some(AuthKind::Middleware("jwtAuth".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("routes.ts"), 42),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("DELETE"));
        assert!(json.contains("jwtAuth"));

        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn http_method_display() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Post.to_string(), "POST");
        assert_eq!(HttpMethod::Delete.to_string(), "DELETE");
    }

    #[test]
    fn auth_kind_decorator_serialization() {
        let iface = Interface {
            method: HttpMethod::Post,
            path: "/api/users".into(),
            auth: Some(AuthKind::Decorator("login_required".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("views.py"), 10),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("login_required"));
        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn auth_kind_annotation_serialization() {
        let iface = Interface {
            method: HttpMethod::Get,
            path: "/api/orders".into(),
            auth: Some(AuthKind::Annotation("PreAuthorize".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("OrderController.java"), 25),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("PreAuthorize"));
        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn auth_kind_attribute_serialization() {
        let iface = Interface {
            method: HttpMethod::Delete,
            path: "/api/items/{id}".into(),
            auth: Some(AuthKind::Attribute("Authorize".into())),
            anchor: SourceAnchor::from_line(PathBuf::from("ItemsController.cs"), 30),
            parameters: vec![],
            handler_name: None,
            request_body_type: None,
        };

        let json = serde_json::to_string(&iface).unwrap();
        assert!(json.contains("Authorize"));
        let deserialized: Interface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, deserialized);
    }

    #[test]
    fn symbol_round_trip_with_all_fields() {
        let symbol = Symbol {
            name: "process_payment".into(),
            kind: SymbolKind::Method,
            anchor: SourceAnchor::from_line_range(PathBuf::from("src/payments.rs"), 42, 60),
            doc: Some("Process a payment transaction.".into()),
            signature: Some("pub fn process_payment(&self, amount: f64) -> Result<Receipt>".into()),
            visibility: Some(Visibility::Public),
            parent: Some("PaymentService".into()),
            is_test: false,
        };

        let json = serde_json::to_string(&symbol).unwrap();
        let deserialized: Symbol = serde_json::from_str(&json).unwrap();
        assert_eq!(symbol, deserialized);

        // Verify serde rename_all works
        assert!(json.contains("\"public\""));
        assert!(json.contains("\"method\""));
    }

    #[test]
    fn symbol_round_trip_with_none_fields() {
        let symbol = Symbol {
            name: "helper".into(),
            kind: SymbolKind::Function,
            anchor: SourceAnchor::from_line_range(PathBuf::from("utils.ts"), 1, 5),
            doc: None,
            signature: None,
            visibility: None,
            parent: None,
            is_test: false,
        };

        let json = serde_json::to_string(&symbol).unwrap();
        let deserialized: Symbol = serde_json::from_str(&json).unwrap();
        assert_eq!(symbol, deserialized);
    }

    #[test]
    fn visibility_all_variants_serialization() {
        for (vis, expected) in [
            (Visibility::Public, "\"public\""),
            (Visibility::Private, "\"private\""),
            (Visibility::Protected, "\"protected\""),
            (Visibility::Internal, "\"internal\""),
        ] {
            let json = serde_json::to_string(&vis).unwrap();
            assert_eq!(json, expected);
            let back: Visibility = serde_json::from_str(&json).unwrap();
            assert_eq!(vis, back);
        }
    }

    #[test]
    fn sink_with_pii_serialization() {
        let sink = Sink {
            sink_type: SinkType::Log,
            anchor: SourceAnchor::from_line(PathBuf::from("handler.ts"), 99),
            text: "logger.info(req.body.password)".into(),
            contains_pii: true,
        };

        let json = serde_json::to_string(&sink).unwrap();
        let deserialized: Sink = serde_json::from_str(&json).unwrap();
        assert_eq!(sink, deserialized);
        assert!(deserialized.contains_pii);
    }

    // --- Knowledge graph type tests ---

    #[test]
    fn reference_round_trip_serialization() {
        let reference = Reference {
            source_symbol: "handle_request".into(),
            source_file: PathBuf::from("src/handler.rs"),
            source_line: 42,
            target_symbol: "validate".into(),
            target_file: Some(PathBuf::from("src/validation.rs")),
            target_line: Some(10),
            reference_kind: ReferenceKind::Call,
            confidence: 0.0,
            resolution_method: ResolutionMethod::Unresolved,
            is_test_reference: false,
        };

        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("\"call\""));
        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(reference, deserialized);
    }

    #[test]
    fn reference_kind_all_variants_serialization() {
        for (kind, expected) in [
            (ReferenceKind::Call, "\"call\""),
            (ReferenceKind::Extends, "\"extends\""),
            (ReferenceKind::Implements, "\"implements\""),
            (ReferenceKind::TypeUsage, "\"type_usage\""),
            (ReferenceKind::Import, "\"import\""),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected);
            let back: ReferenceKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn reference_with_unresolved_target() {
        let reference = Reference {
            source_symbol: "main".into(),
            source_file: PathBuf::from("src/main.ts"),
            source_line: 5,
            target_symbol: "axios.get".into(),
            target_file: None,
            target_line: None,
            reference_kind: ReferenceKind::Call,
            confidence: 0.0,
            resolution_method: ResolutionMethod::Unresolved,
            is_test_reference: false,
        };

        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("null"));
        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(reference, deserialized);
    }

    #[test]
    fn data_model_round_trip_serialization() {
        let model = DataModel {
            name: "User".into(),
            model_kind: DataModelKind::Class,
            fields: vec![
                FieldInfo {
                    name: "id".into(),
                    field_type: Some("number".into()),
                    line: 3,
                    visibility: Some(Visibility::Public),
                },
                FieldInfo {
                    name: "email".into(),
                    field_type: Some("string".into()),
                    line: 4,
                    visibility: Some(Visibility::Private),
                },
            ],
            anchor: SourceAnchor::from_line_range(PathBuf::from("src/models/user.ts"), 2, 10),
            parent_type: Some("BaseEntity".into()),
            implemented_interfaces: vec!["Serializable".into()],
        };

        let json = serde_json::to_string(&model).unwrap();
        assert!(json.contains("\"class\""));
        assert!(json.contains("BaseEntity"));
        let deserialized: DataModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }

    #[test]
    fn data_model_kind_all_variants_serialization() {
        for (kind, expected) in [
            (DataModelKind::Class, "\"class\""),
            (DataModelKind::Struct, "\"struct\""),
            (DataModelKind::Interface, "\"interface\""),
            (DataModelKind::Trait, "\"trait\""),
            (DataModelKind::Enum, "\"enum\""),
            (DataModelKind::Record, "\"record\""),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected);
            let back: DataModelKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn module_boundary_round_trip_serialization() {
        let module = ModuleBoundary {
            name: "payments".into(),
            files: vec![
                PathBuf::from("src/payments/handler.ts"),
                PathBuf::from("src/payments/service.ts"),
            ],
            exported_symbols: vec!["PaymentService".into(), "processPayment".into()],
            depends_on: vec!["users".into(), "orders".into()],
        };

        let json = serde_json::to_string(&module).unwrap();
        let deserialized: ModuleBoundary = serde_json::from_str(&json).unwrap();
        assert_eq!(module, deserialized);
    }

    #[test]
    fn field_info_with_no_type_or_visibility() {
        let field = FieldInfo {
            name: "data".into(),
            field_type: None,
            line: 7,
            visibility: None,
        };

        let json = serde_json::to_string(&field).unwrap();
        let deserialized: FieldInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(field, deserialized);
    }

    // --- FileRole classification tests ---

    #[test]
    fn file_role_classifies_implementation() {
        assert_eq!(
            FileRole::from_path(Path::new("src/engine.rs")),
            FileRole::Implementation
        );
        assert_eq!(
            FileRole::from_path(Path::new("lib/server.ts")),
            FileRole::Implementation
        );
        assert_eq!(
            FileRole::from_path(Path::new("app/models/user.py")),
            FileRole::Implementation
        );
    }

    #[test]
    fn file_role_classifies_tests_by_directory() {
        assert_eq!(
            FileRole::from_path(Path::new("tests/integration.rs")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("src/__tests__/app.test.ts")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("spec/models/user_spec.rb")),
            FileRole::Test
        );
    }

    #[test]
    fn file_role_classifies_tests_by_filename() {
        assert_eq!(
            FileRole::from_path(Path::new("src/app.test.ts")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("src/app.spec.js")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("main_test.go")),
            FileRole::Test
        );
        assert_eq!(
            FileRole::from_path(Path::new("test_models.py")),
            FileRole::Test
        );
    }

    #[test]
    fn file_role_classifies_generated() {
        assert_eq!(
            FileRole::from_path(Path::new("vendor/lib.rs")),
            FileRole::Generated
        );
        assert_eq!(
            FileRole::from_path(Path::new("node_modules/express/index.js")),
            FileRole::Generated
        );
        assert_eq!(
            FileRole::from_path(Path::new("api.generated.ts")),
            FileRole::Generated
        );
        assert_eq!(
            FileRole::from_path(Path::new("service.pb.go")),
            FileRole::Generated
        );
    }

    #[test]
    fn file_role_classifies_build() {
        assert_eq!(
            FileRole::from_path(Path::new("Cargo.toml")),
            FileRole::Build
        );
        assert_eq!(
            FileRole::from_path(Path::new("package.json")),
            FileRole::Build
        );
        assert_eq!(
            FileRole::from_path(Path::new("Dockerfile")),
            FileRole::Build
        );
        assert_eq!(FileRole::from_path(Path::new("Makefile")), FileRole::Build);
    }

    #[test]
    fn file_role_classifies_config() {
        assert_eq!(
            FileRole::from_path(Path::new("config.yaml")),
            FileRole::Config
        );
        assert_eq!(
            FileRole::from_path(Path::new(".env.production")),
            FileRole::Config
        );
        assert_eq!(
            FileRole::from_path(Path::new(".gitignore")),
            FileRole::Config
        );
    }

    #[test]
    fn file_role_classifies_documentation() {
        assert_eq!(
            FileRole::from_path(Path::new("docs/architecture.md")),
            FileRole::Documentation
        );
        assert_eq!(
            FileRole::from_path(Path::new("README.md")),
            FileRole::Documentation
        );
    }

    #[test]
    fn file_role_generated_takes_priority_over_test() {
        // A test file inside vendor/ should be classified as Generated, not Test
        assert_eq!(
            FileRole::from_path(Path::new("vendor/pkg/handler_test.go")),
            FileRole::Generated
        );
    }

    #[test]
    fn file_role_display_matches_as_str() {
        for role in [
            FileRole::Implementation,
            FileRole::Test,
            FileRole::Config,
            FileRole::Documentation,
            FileRole::Generated,
            FileRole::Build,
            FileRole::Other,
        ] {
            assert_eq!(role.to_string(), role.as_str());
        }
    }

    #[test]
    fn file_role_serialization_round_trip() {
        for role in [
            FileRole::Implementation,
            FileRole::Test,
            FileRole::Config,
            FileRole::Documentation,
            FileRole::Generated,
            FileRole::Build,
            FileRole::Other,
        ] {
            let json = serde_json::to_string(&role).unwrap();
            let deserialized: FileRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, deserialized);
        }
    }

    // --- Token estimation tests ---

    #[test]
    fn estimate_tokens_divides_by_four() {
        assert_eq!(estimate_tokens(400), 100);
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(3), 0); // integer division rounds down
        assert_eq!(estimate_tokens(1000), 250);
    }

    // --- CodeModel::filtered() tests ---

    /// Helper: build a minimal CodeModel with the given references.
    fn model_with_refs(refs: Vec<Reference>) -> CodeModel {
        let total = refs.len();
        let resolved = refs.iter().filter(|r| r.confidence > 0.0).count();
        let conf_sum: f64 = refs.iter().map(|r| r.confidence).sum();
        let avg = if total == 0 {
            0.0
        } else {
            conf_sum / total as f64
        };

        CodeModel {
            version: "1.0".into(),
            project_name: "test".into(),
            components: vec![Component {
                name: "default".into(),
                language: SupportedLanguage::TypeScript,
                interfaces: vec![],
                dependencies: vec![],
                sinks: vec![],
                symbols: vec![],
                imports: vec![],
                references: refs,
                data_models: vec![],
                module_boundaries: vec![],
                env_dependencies: vec![],
            }],
            stats: CodeModelStats {
                total_references: total,
                resolved_references: resolved,
                avg_resolution_confidence: avg,
                ..Default::default()
            },
        }
    }

    fn test_ref(confidence: f64) -> Reference {
        Reference {
            source_symbol: "caller".into(),
            source_file: PathBuf::from("src/a.ts"),
            source_line: 1,
            target_symbol: "callee".into(),
            target_file: Some(PathBuf::from("src/b.ts")),
            target_line: Some(1),
            reference_kind: ReferenceKind::Call,
            confidence,
            resolution_method: if confidence > 0.0 {
                ResolutionMethod::ImportBased
            } else {
                ResolutionMethod::Unresolved
            },
            is_test_reference: false,
        }
    }

    #[test]
    fn filtered_removes_low_confidence_references() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.40), test_ref(0.0)]);

        let filtered = model.filtered(0.5);

        assert_eq!(
            filtered.components[0].references.len(),
            1,
            "only the 0.95 ref should survive a 0.5 threshold"
        );
        assert!((filtered.components[0].references[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn filtered_preserves_high_confidence_references() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.80), test_ref(0.50)]);

        let filtered = model.filtered(0.5);

        assert_eq!(
            filtered.components[0].references.len(),
            3,
            "all refs at or above 0.5 should be kept"
        );
    }

    #[test]
    fn filtered_recalculates_stats() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.40), test_ref(0.0)]);

        let filtered = model.filtered(0.5);

        assert_eq!(filtered.stats.total_references, 1);
        assert_eq!(filtered.stats.resolved_references, 1);
        assert!((filtered.stats.avg_resolution_confidence - 0.95).abs() < f64::EPSILON);
    }

    // TODO: integrate with pipeline — test depends on FileTree from file_tree module

    #[test]
    fn filtered_zero_threshold_keeps_all() {
        let model = model_with_refs(vec![test_ref(0.95), test_ref(0.40), test_ref(0.0)]);

        let filtered = model.filtered(0.0);

        assert_eq!(
            filtered.components[0].references.len(),
            3,
            "threshold 0.0 should keep everything"
        );
    }

    #[test]
    fn is_test_reference_preserved_in_serde_round_trip() {
        let mut reference = test_ref(0.90);
        reference.is_test_reference = true;

        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("\"is_test_reference\":true"));
        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_test_reference);
    }

    #[test]
    fn is_test_reference_defaults_to_false_on_deserialize() {
        // Simulate JSON from before the field existed
        let json = r#"{
            "source_symbol":"f",
            "source_file":"a.ts",
            "source_line":1,
            "target_symbol":"g",
            "target_file":null,
            "target_line":null,
            "reference_kind":"call",
            "confidence":0.0,
            "resolution_method":"unresolved"
        }"#;

        let reference: Reference = serde_json::from_str(json).unwrap();
        assert!(
            !reference.is_test_reference,
            "missing field should default to false"
        );
    }

    #[test]
    fn resolution_method_as_str_returns_snake_case() {
        assert_eq!(ResolutionMethod::ImportBased.as_str(), "import_based");
        assert_eq!(ResolutionMethod::SameFile.as_str(), "same_file");
        assert_eq!(ResolutionMethod::GlobalUnique.as_str(), "global_unique");
        assert_eq!(ResolutionMethod::GlobalSameDir.as_str(), "global_same_dir");
        assert_eq!(
            ResolutionMethod::GlobalAmbiguous.as_str(),
            "global_ambiguous"
        );
        assert_eq!(ResolutionMethod::ImportKnown.as_str(), "import_known");
        assert_eq!(ResolutionMethod::External.as_str(), "external");
        assert_eq!(ResolutionMethod::Unresolved.as_str(), "unresolved");
    }
