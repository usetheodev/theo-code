use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionType {
    Read,
    Edit,
    Bash,
    Glob,
    Grep,
    WebFetch,
    Skill,
    Task,
    ExternalDirectory,
}

impl std::fmt::Display for PermissionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Edit => write!(f, "edit"),
            Self::Bash => write!(f, "bash"),
            Self::Glob => write!(f, "glob"),
            Self::Grep => write!(f, "grep"),
            Self::WebFetch => write!(f, "webfetch"),
            Self::Skill => write!(f, "skill"),
            Self::Task => write!(f, "task"),
            Self::ExternalDirectory => write!(f, "external_directory"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub permission: PermissionType,
    pub pattern: String,
    pub action: PermissionAction,
}

#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub permission: PermissionType,
    pub patterns: Vec<String>,
    pub always: Vec<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct PermissionEvaluation {
    pub action: PermissionAction,
}

pub fn evaluate(
    permission: &PermissionType,
    pattern: &str,
    rules: &[PermissionRule],
) -> PermissionEvaluation {
    for rule in rules {
        if rule.permission == *permission {
            let rule_pattern = &rule.pattern;
            if rule_pattern == "*" || glob_match(rule_pattern, pattern) {
                return PermissionEvaluation {
                    action: rule.action.clone(),
                };
            }
        }
    }
    PermissionEvaluation {
        action: PermissionAction::Ask,
    }
}

fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_wildcard_allows_any_pattern() {
        let rules = vec![PermissionRule {
            permission: PermissionType::Bash,
            pattern: "*".to_string(),
            action: PermissionAction::Allow,
        }];
        let result = evaluate(&PermissionType::Bash, "echo hello", &rules);
        assert_eq!(result.action, PermissionAction::Allow);
    }

    #[test]
    fn evaluate_returns_ask_when_no_rules_match() {
        let rules: Vec<PermissionRule> = vec![];
        let result = evaluate(&PermissionType::Read, "file.txt", &rules);
        assert_eq!(result.action, PermissionAction::Ask);
    }

    #[test]
    fn evaluate_deny_rule_takes_effect() {
        let rules = vec![PermissionRule {
            permission: PermissionType::Read,
            pattern: "*.env*".to_string(),
            action: PermissionAction::Deny,
        }];
        let result = evaluate(&PermissionType::Read, "*.env*", &rules);
        assert_eq!(result.action, PermissionAction::Deny);
    }
}
