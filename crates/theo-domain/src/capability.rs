use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::tool::ToolCategory;

/// Defines what an agent is allowed to do.
///
/// Used by CapabilityGate (theo-agent-runtime) to restrict tool usage per agent.
/// - `denied_tools` takes precedence over `allowed_tools` and `allowed_categories`.
/// - Empty `allowed_tools` means all tools are allowed (unless denied).
/// - Empty `allowed_categories` means all categories are allowed (unless denied).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Tool IDs explicitly allowed. Empty = all allowed.
    pub allowed_tools: HashSet<String>,
    /// Tool IDs explicitly denied. Takes precedence over allowed.
    pub denied_tools: HashSet<String>,
    /// Tool categories allowed. Empty = all allowed.
    pub allowed_categories: HashSet<ToolCategory>,
    /// Maximum file size in bytes for write operations.
    pub max_file_size_bytes: u64,
    /// Path prefixes where writing is allowed.
    pub allowed_paths: Vec<String>,
    /// Whether network access is permitted.
    pub network_access: bool,
}

impl CapabilitySet {
    /// No restrictions — all tools, all paths, full network.
    pub fn unrestricted() -> Self {
        Self {
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            allowed_categories: HashSet::new(),
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: true,
        }
    }

    /// Read-only — denies Execution category tools, no write paths.
    pub fn read_only() -> Self {
        let mut denied = HashSet::new();
        denied.insert("bash".to_string());
        denied.insert("edit".to_string());
        denied.insert("write".to_string());
        denied.insert("apply_patch".to_string());

        Self {
            allowed_tools: HashSet::new(),
            denied_tools: denied,
            allowed_categories: HashSet::new(),
            max_file_size_bytes: 0,
            allowed_paths: Vec::new(),
            network_access: false,
        }
    }

    /// Checks if a tool is allowed by this capability set.
    ///
    /// Rules (in order):
    /// 1. If tool_id is in denied_tools → deny
    /// 2. If allowed_tools is non-empty and tool_id is not in it → deny
    /// 3. If allowed_categories is non-empty and category is not in it → deny
    /// 4. Otherwise → allow
    pub fn can_use_tool(&self, tool_id: &str, category: ToolCategory) -> bool {
        // Rule 1: denied takes precedence
        if self.denied_tools.contains(tool_id) {
            return false;
        }

        // Rule 2: if allowlist is set, tool must be in it
        if !self.allowed_tools.is_empty() && !self.allowed_tools.contains(tool_id) {
            return false;
        }

        // Rule 3: if category allowlist is set, category must be in it
        if !self.allowed_categories.is_empty() && !self.allowed_categories.contains(&category) {
            return false;
        }

        true
    }

    /// Checks if writing to a path is allowed.
    ///
    /// Rules:
    /// - If allowed_paths is empty → allow all (unrestricted)
    /// - Otherwise, path must start with one of the allowed prefixes
    pub fn can_write_path(&self, path: &str) -> bool {
        if self.allowed_paths.is_empty() {
            return true;
        }
        self.allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }
}

/// Error returned when a capability check fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDenied {
    pub tool_name: String,
    pub reason: String,
}

impl std::fmt::Display for CapabilityDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "capability denied for '{}': {}",
            self.tool_name, self.reason
        )
    }
}

impl std::error::Error for CapabilityDenied {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrestricted_allows_all_tools() {
        let caps = CapabilitySet::unrestricted();
        assert!(caps.can_use_tool("bash", ToolCategory::Execution));
        assert!(caps.can_use_tool("read", ToolCategory::FileOps));
        assert!(caps.can_use_tool("grep", ToolCategory::Search));
        assert!(caps.can_use_tool("web_fetch", ToolCategory::Web));
    }

    #[test]
    fn unrestricted_allows_all_paths() {
        let caps = CapabilitySet::unrestricted();
        assert!(caps.can_write_path("/any/path"));
        assert!(caps.can_write_path("/etc/passwd"));
    }

    #[test]
    fn read_only_denies_write_tools() {
        let caps = CapabilitySet::read_only();
        assert!(!caps.can_use_tool("bash", ToolCategory::Execution));
        assert!(!caps.can_use_tool("edit", ToolCategory::FileOps));
        assert!(!caps.can_use_tool("write", ToolCategory::FileOps));
        assert!(!caps.can_use_tool("apply_patch", ToolCategory::FileOps));
        // But allows read tools
        assert!(caps.can_use_tool("read", ToolCategory::FileOps));
        assert!(caps.can_use_tool("grep", ToolCategory::Search));
    }

    #[test]
    fn denied_tools_takes_precedence_over_allowed_categories() {
        let mut caps = CapabilitySet::unrestricted();
        caps.denied_tools.insert("bash".to_string());
        // bash is Execution category, but it's explicitly denied
        assert!(!caps.can_use_tool("bash", ToolCategory::Execution));
        // Other execution tools still allowed
        assert!(caps.can_use_tool("other_exec", ToolCategory::Execution));
    }

    #[test]
    fn empty_allowed_tools_allows_all() {
        let caps = CapabilitySet {
            allowed_tools: HashSet::new(), // empty = allow all
            denied_tools: HashSet::new(),
            allowed_categories: HashSet::new(),
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: true,
        };
        assert!(caps.can_use_tool("anything", ToolCategory::Utility));
    }

    #[test]
    fn specific_allowed_tools_restricts() {
        let mut allowed = HashSet::new();
        allowed.insert("read".to_string());
        allowed.insert("grep".to_string());

        let caps = CapabilitySet {
            allowed_tools: allowed,
            denied_tools: HashSet::new(),
            allowed_categories: HashSet::new(),
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: true,
        };
        assert!(caps.can_use_tool("read", ToolCategory::FileOps));
        assert!(caps.can_use_tool("grep", ToolCategory::Search));
        assert!(!caps.can_use_tool("bash", ToolCategory::Execution));
    }

    #[test]
    fn allowed_categories_restricts() {
        let mut cats = HashSet::new();
        cats.insert(ToolCategory::FileOps);
        cats.insert(ToolCategory::Search);

        let caps = CapabilitySet {
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            allowed_categories: cats,
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: true,
        };
        assert!(caps.can_use_tool("read", ToolCategory::FileOps));
        assert!(caps.can_use_tool("grep", ToolCategory::Search));
        assert!(!caps.can_use_tool("bash", ToolCategory::Execution));
        assert!(!caps.can_use_tool("web_fetch", ToolCategory::Web));
    }

    #[test]
    fn can_write_path_with_allowed_prefixes() {
        let caps = CapabilitySet {
            allowed_paths: vec!["/home/user/project/".to_string(), "/tmp/".to_string()],
            ..CapabilitySet::unrestricted()
        };
        assert!(caps.can_write_path("/home/user/project/src/main.rs"));
        assert!(caps.can_write_path("/tmp/test.txt"));
        assert!(!caps.can_write_path("/etc/passwd"));
        assert!(!caps.can_write_path("/home/other/file.rs"));
    }

    #[test]
    fn capability_set_serde_roundtrip() {
        let caps = CapabilitySet::read_only();
        let json = serde_json::to_string(&caps).unwrap();
        let back: CapabilitySet = serde_json::from_str(&json).unwrap();
        assert_eq!(back.denied_tools, caps.denied_tools);
        assert_eq!(back.network_access, caps.network_access);
        assert_eq!(back.max_file_size_bytes, caps.max_file_size_bytes);
    }

    #[test]
    fn capability_denied_display() {
        let denied = CapabilityDenied {
            tool_name: "bash".into(),
            reason: "execution not allowed".into(),
        };
        assert_eq!(
            format!("{}", denied),
            "capability denied for 'bash': execution not allowed"
        );
    }
}
