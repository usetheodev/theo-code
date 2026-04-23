//! YAML frontmatter → `AgentSpec` parser for custom markdown agents.
//!
//! Format:
//! ```markdown
//! ---
//! name: security-reviewer
//! description: "Reviews code for OWASP Top 10"
//! tools: [read, grep, glob]
//! denied_tools: [edit, write, bash]
//! model: claude-sonnet-4-7
//! max_iterations: 25
//! timeout: 300
//! ---
//!
//! You are a security-focused code reviewer. ...
//! ```
//!
//! Numeric fields (A1): `u32` on the wire, cast to `usize`/`u64` in the struct.
//!
//! Track A — Phase 2.

use std::collections::BTreeSet;

use serde::Deserialize;
use thiserror::Error;

use theo_domain::agent_spec::{AgentSpec, AgentSpecSource};
use theo_domain::capability::{AllowedTools, CapabilitySet};

use crate::frontmatter::split_frontmatter;

/// Errors produced by the agent spec parser.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("missing frontmatter (expected leading '---' block)")]
    MissingFrontmatter,
    #[error("invalid YAML frontmatter: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("empty system prompt (body after frontmatter is required)")]
    EmptyBody,
}

/// Raw frontmatter structure (post-YAML parse). Numeric fields are `u32` (A1).
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    #[serde(default)]
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    denied_tools: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    max_iterations: Option<u32>,
    #[serde(default)]
    timeout: Option<u32>,
    /// Optional explicit network access toggle (default: false for safety).
    #[serde(default)]
    network_access: Option<bool>,
}

/// Parse a markdown file containing a YAML frontmatter + system prompt body.
///
/// The caller provides a fallback name (typically the filename minus extension)
/// used when the frontmatter doesn't specify `name:`.
pub fn parse_agent_spec(
    content: &str,
    fallback_name: &str,
    source: AgentSpecSource,
) -> Result<AgentSpec, ParseError> {
    let (fm, body) = split_frontmatter(content).ok_or(ParseError::MissingFrontmatter)?;

    let raw: RawFrontmatter = serde_yaml::from_str(fm)?;

    let description = raw
        .description
        .ok_or(ParseError::MissingField("description"))?;

    let name = raw
        .name
        .unwrap_or_else(|| fallback_name.to_string());

    if body.is_empty() {
        return Err(ParseError::EmptyBody);
    }

    let allowed_tools = match raw.tools {
        None => AllowedTools::All,
        Some(v) if v.is_empty() => AllowedTools::All,
        Some(v) => AllowedTools::Only {
            tools: v.into_iter().collect(),
        },
    };

    let denied_tools: BTreeSet<String> = raw
        .denied_tools
        .unwrap_or_default()
        .into_iter()
        .collect();

    let max_iterations = raw.max_iterations.unwrap_or(30) as usize;
    let timeout_secs = raw.timeout.unwrap_or(300) as u64;
    let network_access = raw.network_access.unwrap_or(false);

    let capability_set = CapabilitySet {
        allowed_tools,
        denied_tools,
        allowed_categories: BTreeSet::new(),
        max_file_size_bytes: u64::MAX,
        allowed_paths: Vec::new(),
        network_access,
    };

    Ok(AgentSpec {
        name,
        description,
        system_prompt: body.to_string(),
        capability_set,
        model_override: raw.model,
        max_iterations,
        timeout_secs,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::tool::ToolCategory;

    #[test]
    fn parse_agent_spec_valid_all_fields() {
        let content = r#"---
name: security-reviewer
description: "Reviews code for OWASP Top 10"
tools:
  - read
  - grep
  - glob
denied_tools:
  - edit
  - write
  - bash
model: claude-sonnet-4-7
max_iterations: 25
timeout: 300
---
You are a security-focused code reviewer."#;

        let spec = parse_agent_spec(content, "fallback", AgentSpecSource::Project).unwrap();
        assert_eq!(spec.name, "security-reviewer");
        assert_eq!(spec.description, "Reviews code for OWASP Top 10");
        assert_eq!(spec.model_override.as_deref(), Some("claude-sonnet-4-7"));
        assert_eq!(spec.max_iterations, 25);
        assert_eq!(spec.timeout_secs, 300);
        assert_eq!(spec.source, AgentSpecSource::Project);
        assert!(spec.capability_set.denied_tools.contains("edit"));
        match &spec.capability_set.allowed_tools {
            AllowedTools::Only { tools } => {
                assert!(tools.contains("read"));
                assert!(tools.contains("grep"));
                assert!(tools.contains("glob"));
            }
            AllowedTools::All => panic!("expected Only"),
        }
    }

    #[test]
    fn parse_agent_spec_minimal_fields_uses_defaults() {
        let content = r#"---
description: "minimal"
---
body content"#;

        let spec = parse_agent_spec(content, "my-agent", AgentSpecSource::Global).unwrap();
        assert_eq!(spec.name, "my-agent"); // falls back to filename
        assert_eq!(spec.description, "minimal");
        assert_eq!(spec.max_iterations, 30); // default
        assert_eq!(spec.timeout_secs, 300); // default
        assert_eq!(spec.model_override, None);
        assert!(!spec.capability_set.network_access); // default: false (safety)
        assert_eq!(spec.capability_set.allowed_tools, AllowedTools::All);
        assert!(spec.capability_set.denied_tools.is_empty());
    }

    #[test]
    fn parse_agent_spec_missing_description_returns_error() {
        let content = r#"---
name: x
---
body"#;
        let err = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap_err();
        match err {
            ParseError::MissingField(field) => assert_eq!(field, "description"),
            other => panic!("expected MissingField, got {:?}", other),
        }
    }

    #[test]
    fn parse_agent_spec_missing_frontmatter_returns_error() {
        let content = "just a body with no frontmatter";
        let err = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap_err();
        assert!(matches!(err, ParseError::MissingFrontmatter));
    }

    #[test]
    fn parse_agent_spec_invalid_yaml_returns_error() {
        let content = "---\n[not valid yaml: : :\n---\nbody";
        let err = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn parse_agent_spec_empty_body_returns_error() {
        let content = "---\ndescription: x\n---\n";
        let err = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap_err();
        assert!(matches!(err, ParseError::EmptyBody));
    }

    #[test]
    fn parse_agent_spec_unknown_fields_ignored() {
        let content = r#"---
description: test
some_weird_field: ignored_value
another_unknown: 42
---
body"#;
        let spec = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap();
        assert_eq!(spec.description, "test");
    }

    #[test]
    fn parse_agent_spec_denied_tools_populates_capability_set() {
        let content = r#"---
description: x
denied_tools: [edit, bash]
---
body"#;
        let spec = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap();
        assert!(spec.capability_set.denied_tools.contains("edit"));
        assert!(spec.capability_set.denied_tools.contains("bash"));
        // Tool checks reflect denied
        assert!(
            !spec
                .capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
    }

    #[test]
    fn parse_agent_spec_empty_tools_array_is_allowed_tools_all() {
        let content = r#"---
description: x
tools: []
---
body"#;
        let spec = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap();
        // Empty tools array → All (consistent with "not specified")
        assert_eq!(spec.capability_set.allowed_tools, AllowedTools::All);
    }

    #[test]
    fn parse_agent_spec_tools_array_populates_allowed_tools() {
        let content = r#"---
description: x
tools: [read, grep]
---
body"#;
        let spec = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap();
        match &spec.capability_set.allowed_tools {
            AllowedTools::Only { tools } => {
                assert!(tools.contains("read"));
                assert!(tools.contains("grep"));
                assert!(!tools.contains("bash"));
            }
            AllowedTools::All => panic!("expected Only"),
        }
    }

    #[test]
    fn parse_agent_spec_name_defaults_to_filename() {
        let content = r#"---
description: x
---
body"#;
        let spec = parse_agent_spec(content, "my-fallback", AgentSpecSource::Project).unwrap();
        assert_eq!(spec.name, "my-fallback");
    }

    #[test]
    fn parse_agent_spec_u32_timeout_cast_to_u64() {
        let content = r#"---
description: x
timeout: 600
---
body"#;
        let spec = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap();
        assert_eq!(spec.timeout_secs, 600u64);
    }

    #[test]
    fn parse_agent_spec_u32_max_iterations_cast_to_usize() {
        let content = r#"---
description: x
max_iterations: 50
---
body"#;
        let spec = parse_agent_spec(content, "x", AgentSpecSource::Project).unwrap();
        assert_eq!(spec.max_iterations, 50usize);
    }

    #[test]
    fn parse_agent_spec_source_preserved() {
        let content = "---\ndescription: x\n---\nbody";
        assert_eq!(
            parse_agent_spec(content, "x", AgentSpecSource::Project)
                .unwrap()
                .source,
            AgentSpecSource::Project
        );
        assert_eq!(
            parse_agent_spec(content, "x", AgentSpecSource::Global)
                .unwrap()
                .source,
            AgentSpecSource::Global
        );
        assert_eq!(
            parse_agent_spec(content, "x", AgentSpecSource::Builtin)
                .unwrap()
                .source,
            AgentSpecSource::Builtin
        );
    }
}
