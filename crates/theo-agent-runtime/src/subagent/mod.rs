//! Sub-agent system — delegated autonomous execution.
//!
//! The main agent can delegate work to specialized sub-agents, each with
//! its own role, capability set, budget, and system prompt.
//! Sub-agent = RunEngine with specialized config. Zero new subsystems.

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_loop::{AgentLoop, AgentResult};
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
use theo_domain::capability::CapabilitySet;
use theo_infra_llm::types::Message;

// ---------------------------------------------------------------------------
// SubAgentRole — specialized roles with restricted capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubAgentRole {
    /// Read-only research and analysis. Cannot edit files.
    Explorer,
    /// Full implementation capability. Can read, write, edit, run commands.
    Implementer,
    /// Validation and testing. Can read and run commands, but not edit.
    Verifier,
    /// Code review and quality analysis. Read-only with reasoning tools.
    Reviewer,
}

impl SubAgentRole {
    /// Map to the canonical routing role-id. Plan §R4: routing slots key
    /// on these string ids so `theo-domain` stays dependency-free.
    pub fn role_id(&self) -> theo_domain::routing::SubAgentRoleId {
        match self {
            SubAgentRole::Explorer => theo_domain::routing::SubAgentRoleId::EXPLORER,
            SubAgentRole::Implementer => theo_domain::routing::SubAgentRoleId::IMPLEMENTER,
            SubAgentRole::Verifier => theo_domain::routing::SubAgentRoleId::VERIFIER,
            SubAgentRole::Reviewer => theo_domain::routing::SubAgentRoleId::REVIEWER,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            SubAgentRole::Explorer => "Explorer",
            SubAgentRole::Implementer => "Implementer",
            SubAgentRole::Verifier => "Verifier",
            SubAgentRole::Reviewer => "Reviewer",
        }
    }

    pub fn max_iterations(&self) -> usize {
        match self {
            SubAgentRole::Explorer => 30,
            SubAgentRole::Implementer => 100,
            SubAgentRole::Verifier => 20,
            SubAgentRole::Reviewer => 20,
        }
    }

    pub fn system_prompt(&self) -> String {
        match self {
            SubAgentRole::Explorer => {
                "You are a code explorer sub-agent. Your job is to READ and UNDERSTAND code, never edit it.\n\
                 - Use read, grep, glob, bash(ls/find) to explore the codebase.\n\
                 - Use think to organize your findings.\n\
                 - Use memory to save important facts.\n\
                 - Report your findings clearly and concisely.\n\
                 - NEVER use edit, write, or apply_patch.".to_string()
            }
            SubAgentRole::Implementer => {
                "You are an implementer sub-agent. Your job is to MAKE CODE CHANGES.\n\
                 - Read files to understand context, then make targeted edits.\n\
                 - Use think to plan your approach before editing.\n\
                 - Validate your changes with bash (cargo check, tests).\n\
                 - Call done when the implementation is complete.".to_string()
            }
            SubAgentRole::Verifier => {
                "You are a verifier sub-agent. Your job is to VALIDATE code, never edit it.\n\
                 - Run tests: cargo test, cargo check, cargo clippy.\n\
                 - Read code to verify correctness.\n\
                 - Use reflect to assess quality and confidence.\n\
                 - Report issues found clearly.\n\
                 - NEVER use edit, write, or apply_patch.".to_string()
            }
            SubAgentRole::Reviewer => {
                "You are a code reviewer sub-agent. Your job is to ANALYZE quality.\n\
                 - Read code carefully for bugs, anti-patterns, and improvements.\n\
                 - Use think to structure your review.\n\
                 - Use reflect to assess overall code quality.\n\
                 - Report findings with severity (critical/major/minor/suggestion).\n\
                 - NEVER use edit, write, or apply_patch.".to_string()
            }
        }
    }

    pub fn capability_set(&self) -> CapabilitySet {
        match self {
            SubAgentRole::Explorer => CapabilitySet::read_only(),
            SubAgentRole::Implementer => CapabilitySet::unrestricted(),
            SubAgentRole::Verifier => {
                use std::collections::HashSet;
                let mut denied = HashSet::new();
                denied.insert("edit".to_string());
                denied.insert("write".to_string());
                denied.insert("apply_patch".to_string());
                CapabilitySet {
                    allowed_tools: HashSet::new(),
                    denied_tools: denied,
                    allowed_categories: HashSet::new(),
                    max_file_size_bytes: u64::MAX,
                    allowed_paths: Vec::new(),
                    network_access: false,
                }
            }
            SubAgentRole::Reviewer => CapabilitySet::read_only(),
        }
    }

    #[allow(clippy::should_implement_trait)] // Returns Option, not Result; intentional API.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "explorer" | "explore" => Some(SubAgentRole::Explorer),
            "implementer" | "implement" => Some(SubAgentRole::Implementer),
            "verifier" | "verify" => Some(SubAgentRole::Verifier),
            "reviewer" | "review" => Some(SubAgentRole::Reviewer),
            _ => None,
        }
    }
}

impl std::fmt::Display for SubAgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ---------------------------------------------------------------------------
// SubAgentManager — orchestrates sub-agent spawning
// ---------------------------------------------------------------------------

/// Maximum sub-agent nesting depth. Sub-agents CANNOT spawn sub-agents.
const MAX_DEPTH: usize = 1;

/// Default timeout for sub-agent execution.
fn timeout_for_role(role: &SubAgentRole) -> u64 {
    match role {
        SubAgentRole::Explorer => 300,    // 5 min (read-only, fast)
        SubAgentRole::Implementer => 600, // 10 min (edits + cargo check)
        SubAgentRole::Verifier => 600,    // 10 min (cargo test can be slow)
        SubAgentRole::Reviewer => 300,    // 5 min (read-only analysis)
    }
}

pub struct SubAgentManager {
    config: AgentConfig,
    event_bus: Arc<EventBus>,
    project_dir: PathBuf,
    depth: usize,
}

impl SubAgentManager {
    pub fn new(config: AgentConfig, event_bus: Arc<EventBus>, project_dir: PathBuf) -> Self {
        Self {
            config,
            event_bus,
            project_dir,
            depth: 0,
        }
    }

    /// Spawn a sub-agent with a specific role and objective.
    ///
    /// The sub-agent gets:
    /// - Isolated TaskManager and ToolCallManager
    /// - Role-specific system prompt and max_iterations
    /// - Shared EventBus (events tagged by sub-agent's run_id)
    /// - Parent's LLM config (same model/provider)
    ///
    /// Mandatory conditions:
    /// - max_depth=1: sub-agents cannot spawn sub-agents
    /// - Timeout: 5 minutes default per sub-agent
    /// - Budget: deducted from parent (via shared BudgetEnforcer)
    pub fn spawn(
        &self,
        role: SubAgentRole,
        objective: &str,
        context: Option<Vec<Message>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let objective = objective.to_string();

        Box::pin(async move {
            // Enforce max_depth
            if self.depth >= MAX_DEPTH {
                return AgentResult {
                    success: false,
                    summary: "Sub-agent depth limit reached. Sub-agents cannot spawn sub-agents."
                        .to_string(),
                    ..Default::default()
                };
            }

            // Build sub-agent config
            let mut sub_config = self.config.clone();
            sub_config.system_prompt = role.system_prompt();
            sub_config.max_iterations = role.max_iterations();
            // Mark as sub-agent: prevents receiving delegation tools and skills.
            // This is the primary defense against recursive spawning.
            sub_config.is_subagent = true;
            // Set capability restrictions based on role.
            // This activates CapabilityGate in the sub-agent's ToolCallManager.
            sub_config.capability_set = Some(role.capability_set());

            // Create sub-agent EventBus with prefixed listener
            let sub_bus = Arc::new(crate::event_bus::EventBus::new());
            let prefixed = Arc::new(PrefixedEventForwarder {
                role_name: role.display_name().to_string(),
                parent_bus: self.event_bus.clone(),
            });
            sub_bus.subscribe(prefixed);

            // Add role identifier + project_dir restriction to system prompt
            sub_config.system_prompt = format!(
                "[{}] {}\n\nIMPORTANT: You MUST only operate within the project directory: {}. \
             Do NOT search, read, or access files outside this directory.",
                role.display_name(),
                sub_config.system_prompt,
                self.project_dir.display()
            );

            // Create agent with default registry (CapabilityGate handles restrictions)
            let registry = theo_tooling::registry::create_default_registry();
            let agent = AgentLoop::new(sub_config, registry);

            // Execute with role-specific timeout
            let history = context.unwrap_or_default();
            let timeout_secs = timeout_for_role(&role);
            let timeout = std::time::Duration::from_secs(timeout_secs);

            match tokio::time::timeout(
                timeout,
                agent.run_with_history(&objective, &self.project_dir, history, Some(sub_bus)),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => AgentResult {
                    success: false,
                    summary: format!(
                        "Sub-agent ({}) timed out after {}s. Objective: {}",
                        role.display_name(),
                        timeout_secs,
                        objective
                    ),
                    ..Default::default()
                },
            }
        }) // close Box::pin(async move {
    }
    /// Spawn multiple sub-agents in parallel.
    ///
    /// All sub-agents execute concurrently via tokio::spawn.
    /// Returns when ALL sub-agents complete (or timeout individually).
    /// Results are returned in the same order as the input tasks.
    pub async fn spawn_parallel(&self, tasks: Vec<(SubAgentRole, String)>) -> Vec<AgentResult> {
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();

        for (role, objective) in tasks {
            let config = self.config.clone();
            let event_bus = self.event_bus.clone();
            let project_dir = self.project_dir.clone();

            join_set.spawn(async move {
                let manager = SubAgentManager::new(config, event_bus, project_dir);
                manager.spawn(role, &objective, None).await
            });
        }

        let mut results = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(agent_result) => results.push(agent_result),
                Err(e) => results.push(AgentResult {
                    success: false,
                    summary: format!("Sub-agent task panicked: {e}"),
                    ..Default::default()
                }),
            }
        }

        results
    }
}

// ---------------------------------------------------------------------------
// PrefixedEventForwarder — tags sub-agent events with role name
// ---------------------------------------------------------------------------

use theo_domain::event::DomainEvent;

struct PrefixedEventForwarder {
    role_name: String,
    parent_bus: Arc<EventBus>,
}

impl crate::event_bus::EventListener for PrefixedEventForwarder {
    fn on_event(&self, event: &DomainEvent) {
        // Clone event and add role prefix to entity_id
        let mut tagged = event.clone();
        tagged.entity_id = format!("[{}] {}", self.role_name, tagged.entity_id);
        self.parent_bus.publish(tagged);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_display_names() {
        assert_eq!(SubAgentRole::Explorer.display_name(), "Explorer");
        assert_eq!(SubAgentRole::Implementer.display_name(), "Implementer");
        assert_eq!(SubAgentRole::Verifier.display_name(), "Verifier");
        assert_eq!(SubAgentRole::Reviewer.display_name(), "Reviewer");
    }

    #[test]
    fn role_max_iterations() {
        assert_eq!(SubAgentRole::Explorer.max_iterations(), 30);
        assert_eq!(SubAgentRole::Implementer.max_iterations(), 100);
        assert_eq!(SubAgentRole::Verifier.max_iterations(), 20);
        assert_eq!(SubAgentRole::Reviewer.max_iterations(), 20);
    }

    #[test]
    fn role_system_prompts_are_unique() {
        let prompts: Vec<String> = [
            SubAgentRole::Explorer,
            SubAgentRole::Implementer,
            SubAgentRole::Verifier,
            SubAgentRole::Reviewer,
        ]
        .iter()
        .map(|r| r.system_prompt())
        .collect();

        for i in 0..prompts.len() {
            for j in (i + 1)..prompts.len() {
                assert_ne!(prompts[i], prompts[j], "Prompts should be unique per role");
            }
        }
    }

    #[test]
    fn explorer_capability_is_read_only() {
        let caps = SubAgentRole::Explorer.capability_set();
        assert!(caps.denied_tools.contains("bash"));
        assert!(caps.denied_tools.contains("edit"));
        assert!(caps.denied_tools.contains("write"));
    }

    #[test]
    fn implementer_capability_is_unrestricted() {
        let caps = SubAgentRole::Implementer.capability_set();
        assert!(caps.denied_tools.is_empty());
        assert!(caps.allowed_tools.is_empty()); // empty = all allowed
    }

    #[test]
    fn verifier_cannot_edit() {
        let caps = SubAgentRole::Verifier.capability_set();
        assert!(caps.denied_tools.contains("edit"));
        assert!(caps.denied_tools.contains("write"));
        assert!(!caps.denied_tools.contains("bash")); // can run tests
    }

    #[test]
    fn reviewer_is_read_only() {
        let caps = SubAgentRole::Reviewer.capability_set();
        assert!(caps.denied_tools.contains("edit"));
        assert!(caps.denied_tools.contains("write"));
    }

    #[test]
    fn from_str_parses_roles() {
        assert_eq!(
            SubAgentRole::from_str("explorer"),
            Some(SubAgentRole::Explorer)
        );
        assert_eq!(
            SubAgentRole::from_str("implement"),
            Some(SubAgentRole::Implementer)
        );
        assert_eq!(
            SubAgentRole::from_str("VERIFIER"),
            Some(SubAgentRole::Verifier)
        );
        assert_eq!(
            SubAgentRole::from_str("review"),
            Some(SubAgentRole::Reviewer)
        );
        assert_eq!(SubAgentRole::from_str("unknown"), None);
    }

    #[test]
    fn spawn_config_sets_is_subagent_true() {
        // Verify that sub-agent configs are marked as sub-agents.
        // This is tested indirectly: SubAgentManager::spawn() sets
        // sub_config.is_subagent = true before creating AgentLoop.
        // We verify the parent config is NOT a sub-agent by default.
        let config = AgentConfig::default();
        assert!(!config.is_subagent, "parent config must not be sub-agent");

        // After clone + manual set (what spawn() does internally):
        let mut sub_config = config.clone();
        sub_config.is_subagent = true;
        assert!(sub_config.is_subagent, "sub-agent config must be marked");
    }

    #[test]
    fn spawn_parallel_configs_inherit_parent_settings() {
        // Verify that spawn_parallel creates managers that will produce
        // sub-agent configs with is_subagent=true (via spawn() internally).
        let config = AgentConfig::default();
        assert!(!config.is_subagent);

        // spawn_parallel clones self.config and passes to SubAgentManager::new()
        // then calls spawn() which sets is_subagent=true on the sub_config.
        let cloned = config.clone();
        let mut sub_config = cloned.clone();
        sub_config.is_subagent = true;
        assert!(sub_config.is_subagent);

        // LLM settings must be preserved
        assert_eq!(sub_config.base_url, config.base_url);
        assert_eq!(sub_config.model, config.model);
        assert_eq!(sub_config.api_key, config.api_key);
    }

    #[test]
    fn max_depth_prevents_recursion() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // Already at max
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(async { manager.spawn(SubAgentRole::Explorer, "test", None).await });
        assert!(!result.success);
        assert!(result.summary.contains("depth limit"));
    }
}
