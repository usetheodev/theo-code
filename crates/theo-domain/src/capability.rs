use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::tool::ToolCategory;

/// Tools that an agent is allowed to use.
///
/// G3 — Replaces the previous `HashSet<String>` with the implicit "empty = all allowed"
/// convention. The implicit convention collided with set-intersection semantics for
/// `CapabilitySet::intersect`: an empty intersection literal `{}` would be reinterpreted
/// as "all allowed", silently escalating capabilities.
///
/// `AllowedTools::All` and `AllowedTools::Only(set)` make the distinction explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum AllowedTools {
    /// All tools are permitted (subject to denied_tools).
    #[default]
    All,
    /// Only the listed tools are permitted (subject to denied_tools).
    Only { tools: BTreeSet<String> },
}

impl AllowedTools {
    /// Convenience constructor for `Only` from any iterable of strings.
    pub fn only<I, S>(tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        AllowedTools::Only {
            tools: tools.into_iter().map(Into::into).collect(),
        }
    }

    /// True if any tool is permitted by this set.
    /// (`All` always; `Only(empty)` never; `Only({...})` for listed tools.)
    pub fn contains(&self, tool: &str) -> bool {
        match self {
            AllowedTools::All => true,
            AllowedTools::Only { tools } => tools.contains(tool),
        }
    }

    /// True iff this is `Only` with an empty set (= deny all).
    pub fn is_empty_only(&self) -> bool {
        matches!(self, AllowedTools::Only { tools } if tools.is_empty())
    }

    /// Set-theoretic intersection.
    ///
    /// - `All ∩ X = X` (identity)
    /// - `Only(A) ∩ Only(B) = Only(A ∩ B)`
    pub fn intersect(&self, other: &AllowedTools) -> AllowedTools {
        match (self, other) {
            (AllowedTools::All, x) | (x, AllowedTools::All) => x.clone(),
            (AllowedTools::Only { tools: a }, AllowedTools::Only { tools: b }) => {
                AllowedTools::Only {
                    tools: a.intersection(b).cloned().collect(),
                }
            }
        }
    }
}


/// Defines what an agent is allowed to do.
///
/// Used by CapabilityGate (theo-agent-runtime) to restrict tool usage per agent.
/// - `denied_tools` takes precedence over `allowed_tools` and `allowed_categories`.
/// - `allowed_tools` is explicit via `AllowedTools` enum (G3): `All` vs `Only(set)`.
/// - Empty `allowed_categories` means all categories are allowed (unless denied).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Tool IDs explicitly allowed (G3 — enum eliminates "empty = all" ambiguity).
    #[serde(default)]
    pub allowed_tools: AllowedTools,
    /// Tool IDs explicitly denied. Takes precedence over allowed.
    /// `BTreeSet` for deterministic ordering across serde roundtrips and intersect.
    #[serde(default)]
    pub denied_tools: BTreeSet<String>,
    /// Tool categories allowed. Empty = all allowed.
    #[serde(default)]
    pub allowed_categories: BTreeSet<ToolCategory>,
    /// Maximum file size in bytes for write operations.
    pub max_file_size_bytes: u64,
    /// Path prefixes where writing is allowed. Empty = all paths allowed.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Whether network access is permitted.
    pub network_access: bool,
}

impl CapabilitySet {
    /// No restrictions — all tools, all paths, full network.
    pub fn unrestricted() -> Self {
        Self {
            allowed_tools: AllowedTools::All,
            denied_tools: BTreeSet::new(),
            allowed_categories: BTreeSet::new(),
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: true,
        }
    }

    /// Read-only — denies write/exec tools, no write paths.
    pub fn read_only() -> Self {
        let mut denied = BTreeSet::new();
        denied.insert("bash".to_string());
        denied.insert("edit".to_string());
        denied.insert("write".to_string());
        denied.insert("apply_patch".to_string());

        Self {
            allowed_tools: AllowedTools::All,
            denied_tools: denied,
            allowed_categories: BTreeSet::new(),
            max_file_size_bytes: 0,
            allowed_paths: Vec::new(),
            network_access: false,
        }
    }

    /// Checks if a tool is allowed by this capability set.
    ///
    /// Rules (in order):
    /// 1. If tool_id is in denied_tools → deny
    /// 2. If allowed_tools is `Only(set)` and tool_id is not in it → deny
    /// 3. If allowed_categories is non-empty and category is not in it → deny
    /// 4. Otherwise → allow
    pub fn can_use_tool(&self, tool_id: &str, category: ToolCategory) -> bool {
        // Rule 1: denied takes precedence
        if self.denied_tools.contains(tool_id) {
            return false;
        }

        // Rule 2: AllowedTools::Only restricts; AllowedTools::All passes
        if !self.allowed_tools.contains(tool_id) {
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

    /// Intersect two capability sets — most restrictive wins (S2 / G2).
    ///
    /// Used when a custom agent overrides a builtin name: the resulting capability set
    /// must never grant more than the builtin already had.
    ///
    /// Rules:
    /// - `denied_tools` = UNION (if either denies, it's denied)
    /// - `allowed_tools` = AllowedTools::intersect (G3)
    /// - `allowed_categories` = INTERSECTION (if both non-empty); empty = inherit from other
    /// - `max_file_size_bytes` = MIN (most restrictive)
    /// - `network_access` = AND (both must allow)
    /// - `allowed_paths` = path-prefix-containment intersection (G2):
    ///   - both empty → empty (= all)
    ///   - only `self` empty → `other` wins (more restrictive)
    ///   - only `other` empty → `self` wins (more restrictive)
    ///   - both non-empty → keep paths in `other` covered by some prefix in `self`
    pub fn intersect(&self, other: &CapabilitySet) -> CapabilitySet {
        let denied_tools: BTreeSet<String> = self
            .denied_tools
            .union(&other.denied_tools)
            .cloned()
            .collect();

        let allowed_tools = self.allowed_tools.intersect(&other.allowed_tools);

        let allowed_categories: BTreeSet<ToolCategory> = match (
            self.allowed_categories.is_empty(),
            other.allowed_categories.is_empty(),
        ) {
            (true, true) => BTreeSet::new(),
            (true, false) => other.allowed_categories.clone(),
            (false, true) => self.allowed_categories.clone(),
            (false, false) => self
                .allowed_categories
                .intersection(&other.allowed_categories)
                .copied()
                .collect(),
        };

        let allowed_paths: Vec<String> = match (
            self.allowed_paths.is_empty(),
            other.allowed_paths.is_empty(),
        ) {
            (true, true) => Vec::new(),
            (true, false) => other.allowed_paths.clone(),
            (false, true) => self.allowed_paths.clone(),
            (false, false) => other
                .allowed_paths
                .iter()
                .filter(|p| {
                    let pb = PathBuf::from(p);
                    self.allowed_paths
                        .iter()
                        .any(|prefix| pb.starts_with(prefix) || p.starts_with(prefix))
                })
                .cloned()
                .collect(),
        };

        CapabilitySet {
            allowed_tools,
            denied_tools,
            allowed_categories,
            max_file_size_bytes: self.max_file_size_bytes.min(other.max_file_size_bytes),
            allowed_paths,
            network_access: self.network_access && other.network_access,
        }
    }

    /// Backward-compat helper: convert a `HashSet<String>` of denied tools into
    /// the new `BTreeSet`-based representation. Useful for legacy call sites.
    pub fn deny_from_hashset(denied: HashSet<String>) -> BTreeSet<String> {
        denied.into_iter().collect()
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

    // ---------------------------------------------------------------------
    // AllowedTools enum (G3)
    // ---------------------------------------------------------------------

    #[test]
    fn allowed_tools_all_intersect_only_returns_only() {
        let only = AllowedTools::only(["read", "grep"]);
        let result = AllowedTools::All.intersect(&only);
        assert_eq!(result, only);
    }

    #[test]
    fn allowed_tools_only_intersect_all_returns_only() {
        let only = AllowedTools::only(["read", "grep"]);
        let result = only.intersect(&AllowedTools::All);
        assert_eq!(result, only);
    }

    #[test]
    fn allowed_tools_only_intersect_only_returns_set_intersection() {
        let a = AllowedTools::only(["read", "grep", "edit"]);
        let b = AllowedTools::only(["read", "write", "edit"]);
        let result = a.intersect(&b);
        match result {
            AllowedTools::Only { tools } => {
                assert!(tools.contains("read"));
                assert!(tools.contains("edit"));
                assert!(!tools.contains("grep"));
                assert!(!tools.contains("write"));
            }
            AllowedTools::All => panic!("expected Only, got All"),
        }
    }

    #[test]
    fn allowed_tools_only_disjoint_returns_empty_only() {
        let a = AllowedTools::only(["read"]);
        let b = AllowedTools::only(["write"]);
        let result = a.intersect(&b);
        match result {
            AllowedTools::Only { tools } => assert!(tools.is_empty()),
            AllowedTools::All => panic!("expected Only(empty), got All"),
        }
    }

    #[test]
    fn allowed_tools_contains_all_returns_true_for_any_tool() {
        assert!(AllowedTools::All.contains("anything"));
        assert!(AllowedTools::All.contains("zzz"));
    }

    #[test]
    fn allowed_tools_contains_only_respects_set() {
        let only = AllowedTools::only(["read", "grep"]);
        assert!(only.contains("read"));
        assert!(only.contains("grep"));
        assert!(!only.contains("bash"));
    }

    #[test]
    fn allowed_tools_default_is_all() {
        assert_eq!(AllowedTools::default(), AllowedTools::All);
    }

    #[test]
    fn allowed_tools_serde_roundtrip_all() {
        let a = AllowedTools::All;
        let json = serde_json::to_string(&a).unwrap();
        let back: AllowedTools = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn allowed_tools_serde_roundtrip_only() {
        let a = AllowedTools::only(["read", "grep"]);
        let json = serde_json::to_string(&a).unwrap();
        let back: AllowedTools = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }

    // ---------------------------------------------------------------------
    // CapabilitySet — existing behavior preserved
    // ---------------------------------------------------------------------

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
    fn empty_allowed_tools_via_all_allows_all() {
        let caps = CapabilitySet {
            allowed_tools: AllowedTools::All,
            denied_tools: BTreeSet::new(),
            allowed_categories: BTreeSet::new(),
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: true,
        };
        assert!(caps.can_use_tool("anything", ToolCategory::Utility));
    }

    #[test]
    fn specific_allowed_tools_restricts() {
        let caps = CapabilitySet {
            allowed_tools: AllowedTools::only(["read", "grep"]),
            denied_tools: BTreeSet::new(),
            allowed_categories: BTreeSet::new(),
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
        let mut cats = BTreeSet::new();
        cats.insert(ToolCategory::FileOps);
        cats.insert(ToolCategory::Search);

        let caps = CapabilitySet {
            allowed_tools: AllowedTools::All,
            denied_tools: BTreeSet::new(),
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
        assert_eq!(back.allowed_tools, caps.allowed_tools);
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

    // ---------------------------------------------------------------------
    // CapabilitySet::intersect (S2 / G2)
    // ---------------------------------------------------------------------

    mod intersect {
        use super::*;

        #[test]
        fn capability_set_intersect_denied_tools_union() {
            let mut a = CapabilitySet::unrestricted();
            a.denied_tools.insert("bash".into());
            let mut b = CapabilitySet::unrestricted();
            b.denied_tools.insert("edit".into());

            let result = a.intersect(&b);
            assert!(result.denied_tools.contains("bash"));
            assert!(result.denied_tools.contains("edit"));
        }

        #[test]
        fn capability_set_intersect_allowed_tools_uses_allowedtools_intersect() {
            let a = CapabilitySet {
                allowed_tools: AllowedTools::only(["read", "grep"]),
                ..CapabilitySet::unrestricted()
            };
            let b = CapabilitySet {
                allowed_tools: AllowedTools::only(["read", "edit"]),
                ..CapabilitySet::unrestricted()
            };
            let result = a.intersect(&b);
            match result.allowed_tools {
                AllowedTools::Only { tools } => {
                    assert_eq!(tools.len(), 1);
                    assert!(tools.contains("read"));
                }
                AllowedTools::All => panic!("expected Only"),
            }
        }

        #[test]
        fn capability_set_intersect_allowed_tools_all_with_only_returns_only() {
            let a = CapabilitySet::unrestricted(); // All
            let b = CapabilitySet {
                allowed_tools: AllowedTools::only(["read"]),
                ..CapabilitySet::unrestricted()
            };
            let result = a.intersect(&b);
            assert_eq!(result.allowed_tools, AllowedTools::only(["read"]));
        }

        #[test]
        fn capability_set_intersect_network_access_and() {
            let a = CapabilitySet {
                network_access: true,
                ..CapabilitySet::unrestricted()
            };
            let b = CapabilitySet {
                network_access: false,
                ..CapabilitySet::unrestricted()
            };
            assert!(!a.intersect(&b).network_access);
            assert!(!b.intersect(&a).network_access);

            let c = CapabilitySet {
                network_access: true,
                ..CapabilitySet::unrestricted()
            };
            assert!(a.intersect(&c).network_access);
        }

        #[test]
        fn capability_set_intersect_max_file_size_min() {
            let a = CapabilitySet {
                max_file_size_bytes: 1_000,
                ..CapabilitySet::unrestricted()
            };
            let b = CapabilitySet {
                max_file_size_bytes: 500,
                ..CapabilitySet::unrestricted()
            };
            assert_eq!(a.intersect(&b).max_file_size_bytes, 500);
            assert_eq!(b.intersect(&a).max_file_size_bytes, 500);
        }

        #[test]
        fn capability_set_intersect_paths_both_empty_returns_empty() {
            let a = CapabilitySet::unrestricted(); // empty paths
            let b = CapabilitySet::unrestricted(); // empty paths
            assert!(a.intersect(&b).allowed_paths.is_empty());
        }

        #[test]
        fn capability_set_intersect_paths_other_empty_returns_self() {
            let a = CapabilitySet {
                allowed_paths: vec!["/a".into()],
                ..CapabilitySet::unrestricted()
            };
            let b = CapabilitySet::unrestricted(); // empty
            let result = a.intersect(&b);
            assert_eq!(result.allowed_paths, vec!["/a".to_string()]);
        }

        #[test]
        fn capability_set_intersect_paths_self_empty_returns_other() {
            let a = CapabilitySet::unrestricted(); // empty
            let b = CapabilitySet {
                allowed_paths: vec!["/b".into()],
                ..CapabilitySet::unrestricted()
            };
            let result = a.intersect(&b);
            assert_eq!(result.allowed_paths, vec!["/b".to_string()]);
        }

        #[test]
        fn capability_set_intersect_paths_prefix_containment() {
            let a = CapabilitySet {
                allowed_paths: vec!["/repo/src".into()],
                ..CapabilitySet::unrestricted()
            };
            let b = CapabilitySet {
                allowed_paths: vec!["/repo/src/lib".into(), "/tmp".into()],
                ..CapabilitySet::unrestricted()
            };
            // intersect = paths in `b` that are under some prefix in `a`
            let result = a.intersect(&b);
            assert!(result.allowed_paths.contains(&"/repo/src/lib".to_string()));
            assert!(!result.allowed_paths.contains(&"/tmp".to_string()));
        }

        #[test]
        fn capability_set_intersect_paths_disjoint_returns_empty() {
            let a = CapabilitySet {
                allowed_paths: vec!["/a".into()],
                ..CapabilitySet::unrestricted()
            };
            let b = CapabilitySet {
                allowed_paths: vec!["/b".into(), "/c".into()],
                ..CapabilitySet::unrestricted()
            };
            assert!(a.intersect(&b).allowed_paths.is_empty());
        }

        #[test]
        fn capability_set_intersect_categories_intersection() {
            let mut cats_a = BTreeSet::new();
            cats_a.insert(ToolCategory::FileOps);
            cats_a.insert(ToolCategory::Search);
            let a = CapabilitySet {
                allowed_categories: cats_a,
                ..CapabilitySet::unrestricted()
            };

            let mut cats_b = BTreeSet::new();
            cats_b.insert(ToolCategory::Search);
            cats_b.insert(ToolCategory::Web);
            let b = CapabilitySet {
                allowed_categories: cats_b,
                ..CapabilitySet::unrestricted()
            };

            let result = a.intersect(&b);
            assert_eq!(result.allowed_categories.len(), 1);
            assert!(result.allowed_categories.contains(&ToolCategory::Search));
        }

        #[test]
        fn capability_set_intersect_is_never_more_permissive_than_self() {
            // S2 invariant: intersect(self, other) capabilities ⊆ self capabilities
            let self_caps = CapabilitySet::read_only();
            let other = CapabilitySet::unrestricted();
            let result = self_caps.intersect(&other);

            // read_only denies bash; result must still deny it
            assert!(!result.can_use_tool("bash", ToolCategory::Execution));
            assert!(!result.can_use_tool("edit", ToolCategory::FileOps));
            assert!(!result.network_access);
        }
    }
}
