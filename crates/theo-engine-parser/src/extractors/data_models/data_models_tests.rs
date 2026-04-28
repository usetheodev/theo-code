//! Sibling test body of `data_models.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `data_models.rs` via `#[path = "data_models_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    #![allow(unused_imports)]
    use std::path::PathBuf;
    use super::*;
    use crate::extractors::data_models::*;
    use crate::tree_sitter::SupportedLanguage;
    use crate::types::{DataModel, DataModelKind, FieldInfo, Visibility};
    

    fn parse_and_extract(source: &str, language: SupportedLanguage) -> Vec<DataModel> {
        let path = PathBuf::from("test_file");
        let parsed = crate::tree_sitter::parse_source(&path, source, language, None).unwrap();
        extract_data_models(source, &parsed.tree, language, &path)
    }

    // 1. TypeScript class with typed fields
    #[test]
    fn typescript_class_with_typed_fields() {
        let models = parse_and_extract(
            r#"
class User {
    public name: string;
    private email: string;
    age: number;
}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "User");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert_eq!(model.fields.len(), 3);

        let name_field = model.fields.iter().find(|f| f.name == "name").unwrap();
        assert_eq!(name_field.field_type.as_deref(), Some("string"));
        assert_eq!(name_field.visibility, Some(Visibility::Public));

        let email_field = model.fields.iter().find(|f| f.name == "email").unwrap();
        assert_eq!(email_field.field_type.as_deref(), Some("string"));
        assert_eq!(email_field.visibility, Some(Visibility::Private));

        let age_field = model.fields.iter().find(|f| f.name == "age").unwrap();
        assert_eq!(age_field.field_type.as_deref(), Some("number"));
    }

    // 2. TypeScript interface with property signatures
    #[test]
    fn typescript_interface_with_property_signatures() {
        let models = parse_and_extract(
            r#"
interface Product {
    id: number;
    title: string;
    price: number;
}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Product");
        assert_eq!(model.model_kind, DataModelKind::Interface);
        assert_eq!(model.fields.len(), 3);

        let id_field = model.fields.iter().find(|f| f.name == "id").unwrap();
        assert_eq!(id_field.field_type.as_deref(), Some("number"));

        let title_field = model.fields.iter().find(|f| f.name == "title").unwrap();
        assert_eq!(title_field.field_type.as_deref(), Some("string"));
    }

    // 3. Python class with __init__ assignments
    #[test]
    fn python_class_with_init_assignments() {
        let models = parse_and_extract(
            r#"
class User:
    def __init__(self, name, email):
        self.name = name
        self.email = email
        self._internal = True
"#,
            SupportedLanguage::Python,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "User");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert_eq!(model.fields.len(), 3);

        let name_field = model.fields.iter().find(|f| f.name == "name").unwrap();
        assert_eq!(name_field.visibility, Some(Visibility::Public));

        let internal_field = model.fields.iter().find(|f| f.name == "_internal").unwrap();
        assert_eq!(internal_field.visibility, Some(Visibility::Private));
    }

    // 4. Java class with field declarations
    #[test]
    fn java_class_with_field_declarations() {
        let models = parse_and_extract(
            r#"
public class Order {
    private String orderId;
    public double total;
    protected String status;
}
"#,
            SupportedLanguage::Java,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Order");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert_eq!(model.fields.len(), 3);

        let order_id = model.fields.iter().find(|f| f.name == "orderId").unwrap();
        assert_eq!(order_id.field_type.as_deref(), Some("String"));
        assert_eq!(order_id.visibility, Some(Visibility::Private));

        let total = model.fields.iter().find(|f| f.name == "total").unwrap();
        assert_eq!(total.field_type.as_deref(), Some("double"));
        assert_eq!(total.visibility, Some(Visibility::Public));

        let status = model.fields.iter().find(|f| f.name == "status").unwrap();
        assert_eq!(status.visibility, Some(Visibility::Protected));
    }

    // 5. Go struct with typed fields
    #[test]
    fn go_struct_with_typed_fields() {
        let models = parse_and_extract(
            r#"
package main

type Server struct {
    Host string
    Port int
    debug bool
}
"#,
            SupportedLanguage::Go,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Server");
        assert_eq!(model.model_kind, DataModelKind::Struct);
        assert_eq!(model.fields.len(), 3);

        let host = model.fields.iter().find(|f| f.name == "Host").unwrap();
        assert_eq!(host.field_type.as_deref(), Some("string"));
        assert_eq!(host.visibility, Some(Visibility::Public));

        let debug = model.fields.iter().find(|f| f.name == "debug").unwrap();
        assert_eq!(debug.field_type.as_deref(), Some("bool"));
        assert_eq!(debug.visibility, Some(Visibility::Private));
    }

    // 6. Rust struct with typed fields
    #[test]
    fn rust_struct_with_typed_fields() {
        let models = parse_and_extract(
            r#"
pub struct Config {
    pub host: String,
    pub port: u16,
    secret: String,
}
"#,
            SupportedLanguage::Rust,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Config");
        assert_eq!(model.model_kind, DataModelKind::Struct);
        assert_eq!(model.fields.len(), 3);

        let host = model.fields.iter().find(|f| f.name == "host").unwrap();
        assert_eq!(host.field_type.as_deref(), Some("String"));
        assert_eq!(host.visibility, Some(Visibility::Public));

        let secret = model.fields.iter().find(|f| f.name == "secret").unwrap();
        assert_eq!(secret.field_type.as_deref(), Some("String"));
        assert_eq!(secret.visibility, Some(Visibility::Private));
    }

    // 7. Class with parent_type detection (extends keyword)
    #[test]
    fn typescript_class_with_extends() {
        let models = parse_and_extract(
            r#"
class Admin extends User {
    role: string;
}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Admin");
        assert_eq!(model.parent_type.as_deref(), Some("User"));
        assert_eq!(model.fields.len(), 1);
    }

    // 8. Empty class returns model with zero fields
    #[test]
    fn empty_class_returns_model_with_zero_fields() {
        let models = parse_and_extract(
            r#"
class Empty {}
"#,
            SupportedLanguage::TypeScript,
        );

        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.name, "Empty");
        assert_eq!(model.model_kind, DataModelKind::Class);
        assert!(model.fields.is_empty());
        assert!(model.parent_type.is_none());
    }
