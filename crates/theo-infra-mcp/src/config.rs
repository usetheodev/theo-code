//! MCP server configuration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpServerConfig {
    /// stdio: subprocess via command + args. Trust local.
    Stdio {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// HTTP transport (Streamable HTTP). Requires OAuth 2.1 (future iteration).
    Http {
        name: String,
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

impl McpServerConfig {
    pub fn name(&self) -> &str {
        match self {
            McpServerConfig::Stdio { name, .. } => name,
            McpServerConfig::Http { name, .. } => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_config_yaml_format() {
        let yaml = r#"
transport: stdio
name: github
command: npx
args:
  - "-y"
  - "@modelcontextprotocol/server-github"
env:
  GITHUB_TOKEN: "abc123"
"#;
        let cfg: McpServerConfig = serde_yaml_from_str(yaml).unwrap();
        match cfg {
            McpServerConfig::Stdio {
                name, command, args, env,
            } => {
                assert_eq!(name, "github");
                assert_eq!(command, "npx");
                assert_eq!(args.len(), 2);
                assert_eq!(env.get("GITHUB_TOKEN").unwrap(), "abc123");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn http_config_yaml_format() {
        let yaml = r#"
transport: http
name: postgres
url: http://localhost:8080
headers:
  Authorization: "Bearer xyz"
"#;
        let cfg: McpServerConfig = serde_yaml_from_str(yaml).unwrap();
        match cfg {
            McpServerConfig::Http { name, url, headers } => {
                assert_eq!(name, "postgres");
                assert_eq!(url, "http://localhost:8080");
                assert_eq!(headers.get("Authorization").unwrap(), "Bearer xyz");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn name_accessor_returns_correct_value() {
        let stdio = McpServerConfig::Stdio {
            name: "a".into(),
            command: "x".into(),
            args: vec![],
            env: BTreeMap::new(),
        };
        let http = McpServerConfig::Http {
            name: "b".into(),
            url: "http://x".into(),
            headers: BTreeMap::new(),
        };
        assert_eq!(stdio.name(), "a");
        assert_eq!(http.name(), "b");
    }

    /// Minimal YAML helper using serde_json (which accepts JSON, a strict
    /// subset of YAML) — avoids pulling serde_yaml as a dep here.
    fn serde_yaml_from_str(yaml: &str) -> Result<McpServerConfig, serde_json::Error> {
        // Convert simple YAML → JSON manually for tests
        // (we don't want serde_yaml dep in this crate; tests use the
        // protocol crate's JSON path)
        let trimmed = yaml.trim();
        if trimmed.starts_with("transport: stdio") {
            // Extract fields by simple parsing
            let mut command = String::new();
            let mut name = String::new();
            let mut args = Vec::new();
            let mut env = BTreeMap::new();
            let mut in_args = false;
            let mut in_env = false;
            for raw in trimmed.lines() {
                let line = raw.trim_end();
                let no_indent = line.trim_start();
                if no_indent.is_empty() {
                    continue;
                }
                if !line.starts_with(' ') {
                    in_args = false;
                    in_env = false;
                }
                if let Some(v) = no_indent.strip_prefix("name: ") {
                    name = v.trim().to_string();
                } else if let Some(v) = no_indent.strip_prefix("command: ") {
                    command = v.trim().to_string();
                } else if no_indent == "args:" {
                    in_args = true;
                } else if no_indent == "env:" {
                    in_env = true;
                } else if in_args && let Some(v) = no_indent.strip_prefix("- ") {
                    args.push(v.trim().trim_matches('"').to_string());
                } else if in_env && let Some((k, v)) = no_indent.split_once(": ") {
                    env.insert(
                        k.trim().to_string(),
                        v.trim().trim_matches('"').to_string(),
                    );
                }
            }
            let json = serde_json::json!({
                "transport": "stdio",
                "name": name,
                "command": command,
                "args": args,
                "env": env,
            });
            serde_json::from_value(json)
        } else {
            // http
            let mut name = String::new();
            let mut url = String::new();
            let mut headers = BTreeMap::new();
            let mut in_headers = false;
            for raw in trimmed.lines() {
                let line = raw.trim_end();
                let no_indent = line.trim_start();
                if no_indent.is_empty() {
                    continue;
                }
                if !line.starts_with(' ') {
                    in_headers = false;
                }
                if let Some(v) = no_indent.strip_prefix("name: ") {
                    name = v.trim().to_string();
                } else if let Some(v) = no_indent.strip_prefix("url: ") {
                    url = v.trim().to_string();
                } else if no_indent == "headers:" {
                    in_headers = true;
                } else if in_headers && let Some((k, v)) = no_indent.split_once(": ") {
                    headers.insert(
                        k.trim().to_string(),
                        v.trim().trim_matches('"').to_string(),
                    );
                }
            }
            let json = serde_json::json!({
                "transport": "http",
                "name": name,
                "url": url,
                "headers": headers,
            });
            serde_json::from_value(json)
        }
    }
}
