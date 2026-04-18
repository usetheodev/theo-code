//! Streaming JSON parser for incremental LLM tool-call arguments.
//!
//! When an LLM streams tool call arguments character by character, we want to
//! extract partial information (like file paths) before the full response
//! arrives. This module tries to "close" incomplete JSON by appending missing
//! brackets, braces, and quotes.
//!
//! Reference: pi-mono `packages/ai/src/utils/json-parse.ts` (uses `partial-json` npm package).
//! We reimplement the closing strategy in pure Rust using only `serde_json`.

use serde_json::Value;

/// Attempt to parse a potentially incomplete JSON string.
///
/// Strategy:
/// 1. Fast path — try `serde_json::from_str` directly.
/// 2. Slow path — walk the input to figure out which closers are missing,
///    then append them and try again.
///
/// Returns `None` for empty/whitespace-only input or when no closing strategy
/// produces valid JSON.
pub fn parse_partial_json(input: &str) -> Option<Value> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Fast path: already valid JSON.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }

    // Slow path: compute the closing suffix and try to parse.
    let closed = close_json(trimmed)?;
    serde_json::from_str::<Value>(&closed).ok()
}

/// Walk the input tracking structural state, then append the necessary closing
/// characters. Returns `None` if the input doesn't start with a JSON structural
/// character (no hope of recovery).
fn close_json(input: &str) -> Option<String> {
    // We need at least one structural opener to attempt closing.
    let first = input.trim_start().chars().next()?;
    if !matches!(first, '{' | '[' | '"') {
        // Could be a partial number/bool/null — try as-is, caller already
        // attempted full parse so nothing more we can do.
        return None;
    }

    let mut closers: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape_next = false;

    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        if ch == '\\' && in_string {
            escape_next = true;
            i += 1;
            continue;
        }

        if ch == '"' {
            if in_string {
                in_string = false;
                // Pop the matching `"` closer if present.
                if closers.last() == Some(&'"') {
                    closers.pop();
                }
            } else {
                in_string = true;
                closers.push('"');
            }
            i += 1;
            continue;
        }

        if in_string {
            i += 1;
            continue;
        }

        match ch {
            '{' => closers.push('}'),
            '[' => closers.push(']'),
            '}' | ']' => {
                // Pop the matching opener's closer.
                closers.pop();
            }
            _ => {}
        }

        i += 1;
    }

    // Build the closed string.
    // If we are mid-string, the last closer is `"`, which we'll emit.
    // After that we need all remaining structural closers in reverse order.
    let mut result = String::with_capacity(input.len() + closers.len());
    result.push_str(input);

    // If we ended mid-escape (e.g. `{"a":"\`), drop the trailing backslash
    // before closing the string so the result is valid.
    if escape_next {
        result.pop(); // remove dangling `\`
    }

    for &closer in closers.iter().rev() {
        result.push(closer);
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- Complete JSON (fast path) ----

    #[test]
    fn test_complete_json_object() {
        let input = r#"{"filePath":"a.rs"}"#;
        let result = parse_partial_json(input);
        assert_eq!(result, Some(json!({"filePath": "a.rs"})));
    }

    #[test]
    fn test_complete_json_array() {
        let input = r#"[1, 2, 3]"#;
        let result = parse_partial_json(input);
        assert_eq!(result, Some(json!([1, 2, 3])));
    }

    // ---- Empty / whitespace ----

    #[test]
    fn test_empty_string_returns_none() {
        assert_eq!(parse_partial_json(""), None);
    }

    #[test]
    fn test_whitespace_only_returns_none() {
        assert_eq!(parse_partial_json("   \n\t  "), None);
    }

    // ---- Malformed / unrecoverable ----

    #[test]
    fn test_malformed_returns_none() {
        assert_eq!(parse_partial_json("not json at all"), None);
    }

    // ---- Partial object: missing closing brace ----

    #[test]
    fn test_partial_object_missing_brace() {
        let input = r#"{"filePath":"a.rs""#;
        let result = parse_partial_json(input);
        assert_eq!(result, Some(json!({"filePath": "a.rs"})));
    }

    // ---- Partial value: string value not yet closed ----

    #[test]
    fn test_partial_string_value() {
        let input = r#"{"filePath":"a."#;
        let result = parse_partial_json(input);
        // Best effort: we close the string and the object.
        let v = result.expect("should parse partial string value");
        assert_eq!(v["filePath"], json!("a."));
    }

    // ---- Nested partial objects ----

    #[test]
    fn test_nested_partial_object() {
        let input = r#"{"a":{"b":1"#;
        let result = parse_partial_json(input);
        let v = result.expect("should parse nested partial");
        assert_eq!(v["a"]["b"], json!(1));
    }

    #[test]
    fn test_deeply_nested_partial() {
        let input = r#"{"a":{"b":{"c":"val"#;
        let result = parse_partial_json(input);
        let v = result.expect("should parse deeply nested partial");
        assert_eq!(v["a"]["b"]["c"], json!("val"));
    }

    // ---- Partial array ----

    #[test]
    fn test_partial_array() {
        let input = r#"[1, 2"#;
        let result = parse_partial_json(input);
        assert_eq!(result, Some(json!([1, 2])));
    }

    #[test]
    fn test_partial_array_with_trailing_comma() {
        // `[1, 2,` — closing with `]` gives `[1, 2,]` which is invalid JSON.
        // This is a known limitation; serde_json rejects trailing commas.
        let input = r#"[1, 2,"#;
        let result = parse_partial_json(input);
        // May or may not parse depending on trailing-comma tolerance.
        // We accept None here as a valid outcome.
        let _ = result;
    }

    // ---- Partial with escape sequences ----

    #[test]
    fn test_partial_with_escaped_quote() {
        let input = r#"{"msg":"hello \"world"#;
        let result = parse_partial_json(input);
        let v = result.expect("should handle escaped quotes");
        // The value should contain the escaped quote.
        let msg = v["msg"].as_str().unwrap();
        assert!(msg.contains("world"));
    }

    // ---- Partial key (no value yet) ----

    #[test]
    fn test_partial_key_only() {
        // `{"file` — close string + close object → `{"file"}`
        // serde_json won't accept a key without value, so this should be None.
        let input = r#"{"file"#;
        let result = parse_partial_json(input);
        // A bare key without `:` and value is invalid JSON even when closed.
        // We accept None.
        assert!(result.is_none());
    }

    // ---- Already-closed partial still works ----

    #[test]
    fn test_partial_with_one_complete_and_one_partial_key() {
        let input = r#"{"done":true,"partial":"val"#;
        let result = parse_partial_json(input);
        let v = result.expect("should parse mixed complete/partial");
        assert_eq!(v["done"], json!(true));
        assert_eq!(v["partial"], json!("val"));
    }

    // ---- Trailing backslash edge case ----

    #[test]
    fn test_trailing_backslash_in_string() {
        let input = r#"{"path":"c:\"#;
        let result = parse_partial_json(input);
        // The trailing `\` would escape our closing `"`, so we drop it.
        // Result: `{"path":"c:"}` which is valid.
        let v = result.expect("should handle trailing backslash");
        assert_eq!(v["path"], json!("c:"));
    }

    // ---- Bare string ----

    #[test]
    fn test_partial_bare_string() {
        let input = r#""hello"#;
        let result = parse_partial_json(input);
        assert_eq!(result, Some(json!("hello")));
    }

    // ---- Mixed arrays and objects ----

    #[test]
    fn test_partial_array_of_objects() {
        let input = r#"[{"a":1},{"b":2"#;
        let result = parse_partial_json(input);
        let v = result.expect("should parse partial array of objects");
        assert_eq!(v[0]["a"], json!(1));
        assert_eq!(v[1]["b"], json!(2));
    }
}
