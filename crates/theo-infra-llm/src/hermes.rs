use crate::types::ToolCall;

/// Parse Hermes XML format tool calls from LLM content.
///
/// Some local models (e.g. via vLLM) emit tool calls as XML in the content
/// instead of using the structured tool_calls field:
///
/// ```text
/// <function=edit_file>
/// <parameter=path>src/main.py</parameter>
/// <parameter=new_text>print("hello")</parameter>
/// </function>
/// ```
pub fn parse_hermes_tool_calls(content: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut search_from = 0;

    while let Some(func_start) = content[search_from..].find("<function=") {
        let abs_start = search_from + func_start;
        let after_tag = abs_start + "<function=".len();

        // Extract function name
        let Some(name_end) = content[after_tag..].find('>') else {
            break;
        };
        let name = content[after_tag..after_tag + name_end].trim().to_string();

        // Find closing </function>
        let Some(func_end_rel) = content[after_tag..].find("</function>") else {
            break;
        };
        let body = &content[after_tag + name_end + 1..after_tag + func_end_rel];

        // Extract parameters
        let mut params = serde_json::Map::new();
        let mut param_search = 0;
        while let Some(param_start) = body[param_search..].find("<parameter=") {
            let p_abs = param_search + param_start;
            let p_after = p_abs + "<parameter=".len();

            let Some(pname_end) = body[p_after..].find('>') else {
                break;
            };
            let param_name = body[p_after..p_after + pname_end].trim().to_string();

            let p_value_start = p_after + pname_end + 1;
            let value_end = body[p_value_start..]
                .find("</parameter>")
                .unwrap_or(body.len() - p_value_start);
            let param_value = body[p_value_start..p_value_start + value_end]
                .trim()
                .to_string();

            params.insert(param_name, serde_json::Value::String(param_value));
            param_search = p_value_start + value_end;
        }

        let id = format!("hermes_{}", calls.len());
        let arguments = serde_json::to_string(&params).unwrap_or_default();
        calls.push(ToolCall::new(id, name, arguments));

        search_from = after_tag + func_end_rel + "</function>".len();
    }

    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_tool_call() {
        let content = r#"I'll read the file.
<function=read_file>
<parameter=path>src/main.py</parameter>
</function>"#;

        let calls = parse_hermes_tool_calls(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");

        let args: serde_json::Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
        assert_eq!(args["path"], "src/main.py");
    }

    #[test]
    fn test_parse_multiple_parameters() {
        let content = r#"<function=edit_file>
<parameter=path>src/main.py</parameter>
<parameter=old_text>print("hello")</parameter>
<parameter=new_text>print("world")</parameter>
</function>"#;

        let calls = parse_hermes_tool_calls(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "edit_file");

        let args: serde_json::Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
        assert_eq!(args["path"], "src/main.py");
        assert_eq!(args["old_text"], "print(\"hello\")");
        assert_eq!(args["new_text"], "print(\"world\")");
    }

    #[test]
    fn test_parse_multiple_calls() {
        let content = r#"Let me do two things.
<function=read_file>
<parameter=path>a.py</parameter>
</function>
<function=read_file>
<parameter=path>b.py</parameter>
</function>"#;

        let calls = parse_hermes_tool_calls(content);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "hermes_0");
        assert_eq!(calls[1].id, "hermes_1");
    }

    #[test]
    fn test_no_tool_calls() {
        let content = "Just a regular response with no tool calls.";
        let calls = parse_hermes_tool_calls(content);
        assert!(calls.is_empty());
    }
}
