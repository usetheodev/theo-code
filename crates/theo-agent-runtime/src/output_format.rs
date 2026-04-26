//! Output format schema for sub-agents (D3 enforcement opcional).
//!
//! Track B — 
//!
//! Permite que `AgentSpec` declare um JSON schema para validar/parsear o
//! output do sub-agent. Modos:
//! - `Strict`: parser FALHA se output nao bate o schema. Sub-agent retorna erro.
//! - `BestEffort` (default): parser tenta, se falha mantem free-text e
//!   `AgentResult.structured = None`.
//!
//! Reference: Archon dag-node.ts `output_format` (Claude/Codex SDK enforcement,
//! Pi best_effort via prompt augmentation + JSON extraction).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Enforcement mode for the output schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    /// Try to parse + validate; on failure keep free-text and set structured=None.
    #[default]
    BestEffort,
    /// Parser failure → AgentResult.success = false.
    Strict,
}

/// Optional structured output schema attached to an AgentSpec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputFormat {
    /// JSON Schema (Draft 7) describing the expected output shape.
    pub schema: serde_json::Value,
    #[serde(default)]
    pub enforcement: Enforcement,
}

/// Error from output validation.
#[derive(Debug, Error)]
pub enum OutputError {
    #[error("output is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("output does not match schema: {reason}")]
    SchemaMismatch { reason: String },
}

/// Try to parse a free-text summary into a structured JSON value.
///
/// Strategy:
/// 1. Locate a JSON object/array in `text` (greedy: first `{` or `[` to
///    matching close — handles cases where the LLM wraps JSON in prose).
/// 2. Parse with `serde_json`.
/// 3. Run schema validation (lightweight: type + required fields only).
///
/// Returns `Ok(value)` on success or `Err` on parse / schema failure.
pub fn try_parse_structured(text: &str, schema: &serde_json::Value) -> Result<serde_json::Value, OutputError> {
    let json_str = extract_first_json_object(text).ok_or_else(|| OutputError::SchemaMismatch {
        reason: "no JSON object/array found in output".to_string(),
    })?;
    let value: serde_json::Value = serde_json::from_str(json_str)?;
    validate_against_schema(&value, schema).map(|_| value)
}

/// Greedy extraction of the first balanced JSON object or array.
/// Returns the substring containing the JSON, or None if not found.
fn extract_first_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{' || b == b'[')?;
    let opener = bytes[start];
    let closer = if opener == b'{' { b'}' } else { b']' };
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if b == opener {
            depth += 1;
        } else if b == closer {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..=i]);
            }
        }
    }
    None
}

/// Lightweight schema validation: checks `type` and `required` fields only.
/// This is intentionally minimal — full JSON Schema validation would require
/// pulling in a heavy dep (jsonschema crate ~1.5MB).
///
/// Returns `Ok(())` if valid, `Err(OutputError::SchemaMismatch)` otherwise.
pub fn validate_against_schema(
    value: &serde_json::Value,
    schema: &serde_json::Value,
) -> Result<(), OutputError> {
    if let Some(expected_type) = schema.get("type").and_then(|v| v.as_str()) {
        let actual_type = json_type(value);
        // JSON Schema "integer" accepts numbers without fractional part
        let matches = actual_type == expected_type
            || (expected_type == "integer"
                && matches!(value.as_f64(), Some(n) if n.fract() == 0.0));
        if !matches {
            return Err(OutputError::SchemaMismatch {
                reason: format!("expected type {}, got {}", expected_type, actual_type),
            });
        }
    }
    if let Some(required) = schema.get("required").and_then(|v| v.as_array())
        && let Some(obj) = value.as_object() {
            for field in required {
                if let Some(name) = field.as_str()
                    && !obj.contains_key(name) {
                        return Err(OutputError::SchemaMismatch {
                            reason: format!("missing required field: {}", name),
                        });
                    }
            }
        }
    // Recursive: validate items in arrays
    if let Some(items_schema) = schema.get("items")
        && let Some(arr) = value.as_array() {
            for (i, item) in arr.iter().enumerate() {
                validate_against_schema(item, items_schema).map_err(|e| OutputError::SchemaMismatch {
                    reason: format!("items[{}]: {}", i, e),
                })?;
            }
        }
    // Recursive: validate properties of objects
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object())
        && let Some(obj) = value.as_object() {
            for (key, field_schema) in props {
                if let Some(field_value) = obj.get(key) {
                    validate_against_schema(field_value, field_schema)
                        .map_err(|e| OutputError::SchemaMismatch {
                            reason: format!("{}: {}", key, e),
                        })?;
                }
            }
        }
    // Recursive: validate enum
    if let Some(allowed) = schema.get("enum").and_then(|v| v.as_array())
        && !allowed.contains(value) {
            return Err(OutputError::SchemaMismatch {
                reason: format!("value not in enum: {}", value),
            });
        }
    Ok(())
}

fn json_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["findings"],
            "properties": {
                "findings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["severity", "file"],
                        "properties": {
                            "severity": { "enum": ["critical", "high", "medium", "low"] },
                            "file": { "type": "string" },
                            "line": { "type": "number" }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn enforcement_default_is_best_effort() {
        assert_eq!(Enforcement::default(), Enforcement::BestEffort);
    }

    #[test]
    fn try_parse_structured_strict_valid_returns_value() {
        let schema = fixture_schema();
        let text = r#"{"findings": [{"severity": "high", "file": "x.rs"}]}"#;
        let value = try_parse_structured(text, &schema).unwrap();
        assert_eq!(value["findings"][0]["severity"], "high");
    }

    #[test]
    fn try_parse_structured_extracts_json_from_surrounding_prose() {
        let schema = fixture_schema();
        let text = r#"Here is my analysis:
{"findings": [{"severity": "low", "file": "y.rs"}]}
Hope this helps!"#;
        let value = try_parse_structured(text, &schema).unwrap();
        assert_eq!(value["findings"][0]["file"], "y.rs");
    }

    #[test]
    fn try_parse_structured_no_json_returns_error() {
        let schema = fixture_schema();
        let err = try_parse_structured("just plain text", &schema).unwrap_err();
        match err {
            OutputError::SchemaMismatch { reason } => assert!(reason.contains("no JSON")),
            _ => panic!(),
        }
    }

    #[test]
    fn try_parse_structured_invalid_json_returns_error() {
        let schema = fixture_schema();
        let err = try_parse_structured("{invalid", &schema).unwrap_err();
        // unbalanced → no JSON found
        assert!(matches!(err, OutputError::SchemaMismatch { .. }));
    }

    #[test]
    fn try_parse_structured_missing_required_field_fails() {
        let schema = fixture_schema();
        let text = r#"{"other": "something"}"#;
        let err = try_parse_structured(text, &schema).unwrap_err();
        match err {
            OutputError::SchemaMismatch { reason } => {
                assert!(reason.contains("findings"), "{}", reason)
            }
            _ => panic!(),
        }
    }

    #[test]
    fn try_parse_structured_wrong_type_fails() {
        let schema = serde_json::json!({"type": "object"});
        let err = try_parse_structured("[1, 2, 3]", &schema).unwrap_err();
        match err {
            OutputError::SchemaMismatch { reason } => {
                assert!(reason.contains("expected type object"))
            }
            _ => panic!(),
        }
    }

    #[test]
    fn try_parse_structured_invalid_enum_fails() {
        let schema = fixture_schema();
        let text = r#"{"findings": [{"severity": "trivial", "file": "x.rs"}]}"#;
        let err = try_parse_structured(text, &schema).unwrap_err();
        match err {
            OutputError::SchemaMismatch { reason } => {
                assert!(reason.contains("enum"), "{}", reason)
            }
            _ => panic!(),
        }
    }

    #[test]
    fn extract_first_json_handles_nested_objects() {
        let text = r#"prefix {"a": {"b": "c"}} suffix"#;
        let extracted = extract_first_json_object(text).unwrap();
        assert_eq!(extracted, r#"{"a": {"b": "c"}}"#);
    }

    #[test]
    fn extract_first_json_handles_nested_arrays() {
        let text = r#"x [[1, 2], [3, 4]] y"#;
        let extracted = extract_first_json_object(text).unwrap();
        assert_eq!(extracted, r#"[[1, 2], [3, 4]]"#);
    }

    #[test]
    fn extract_first_json_handles_strings_with_braces() {
        let text = r#"{"msg": "hello {world}"}"#;
        let extracted = extract_first_json_object(text).unwrap();
        assert_eq!(extracted, r#"{"msg": "hello {world}"}"#);
    }

    #[test]
    fn output_format_serde_roundtrip() {
        let f = OutputFormat {
            schema: fixture_schema(),
            enforcement: Enforcement::Strict,
        };
        let json = serde_json::to_string(&f).unwrap();
        let back: OutputFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(back.enforcement, Enforcement::Strict);
    }

    #[test]
    fn validate_against_schema_recursive_property_ok() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "required": ["x"],
                    "properties": { "x": { "type": "number" } }
                }
            }
        });
        let value = serde_json::json!({"nested": {"x": 42}});
        assert!(validate_against_schema(&value, &schema).is_ok());
    }

    #[test]
    fn validate_against_schema_recursive_property_fails_deep() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "required": ["x"],
                    "properties": { "x": { "type": "number" } }
                }
            }
        });
        let value = serde_json::json!({"nested": {"y": 42}}); // missing x
        let err = validate_against_schema(&value, &schema).unwrap_err();
        match err {
            OutputError::SchemaMismatch { reason } => assert!(reason.contains("x")),
            _ => panic!(),
        }
    }
}
