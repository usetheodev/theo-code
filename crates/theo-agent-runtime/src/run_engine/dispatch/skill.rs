//! `skill` meta-tool handler.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from `run_engine/mod.rs`.
//! Two skill modes:
//!   - `InContext`: inject instructions as a system message.
//!   - `SubAgent { agent_name }`: spawn a sub-agent carrying the
//!     skill's instructions as the objective.

use std::sync::Arc;

use theo_domain::event::{DomainEvent, EventType};
use theo_infra_llm::types::{Message, ToolCall};

use crate::run_engine::AgentRunEngine;
use crate::skill::SkillRegistry;

impl AgentRunEngine {
    /// Dispatch a `skill` tool call. Pushes the tool result into
    /// `messages`; may also push a system message (InContext mode).
    pub(in crate::run_engine) async fn dispatch_skill(
        &mut self,
        call: &ToolCall,
        messages: &mut Vec<Message>,
    ) {
        let args = call.parse_arguments().unwrap_or_default();
        let skill_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");

        // Temporary registry for lookup.
        let mut skill_registry = SkillRegistry::new();
        skill_registry.load_bundled();
        let project_skills = self.project_dir.join(".theo").join("skills");
        if project_skills.exists() {
            skill_registry.load_from_dir(&project_skills);
        }

        let Some(skill) = skill_registry.get(skill_name) else {
            let available: Vec<String> = skill_registry
                .list()
                .iter()
                .map(|s| s.name.clone())
                .collect();
            messages.push(Message::tool_result(
                &call.id,
                "skill",
                format!(
                    "Unknown skill: '{}'. Available skills: {}",
                    skill_name,
                    available.join(", ")
                ),
            ));
            return;
        };

        match &skill.mode {
            crate::skill::SkillMode::InContext => {
                // Inject skill instructions into the conversation.
                messages.push(Message::system(&skill.instructions));
                messages.push(Message::tool_result(
                    &call.id,
                    "skill",
                    format!(
                        "Skill '{}' loaded. Follow the instructions above.",
                        skill_name
                    ),
                ));
            }
            crate::skill::SkillMode::SubAgent { agent_name } => {
                self.spawn_skill_subagent(call, messages, skill_name, agent_name, &skill.instructions)
                    .await;
            }
        }
    }

    async fn spawn_skill_subagent(
        &mut self,
        call: &ToolCall,
        messages: &mut Vec<Message>,
        skill_name: &str,
        agent_name: &str,
        instructions: &str,
    ) {
        self.event_bus.publish(DomainEvent::new(
            EventType::RunStateChanged,
            self.run.run_id.as_str(),
            serde_json::json!({
                "from": "Executing",
                "to": format!("Skill:{}:{}", skill_name, agent_name),
            }),
        ));

        let registry: Arc<crate::subagent::SubAgentRegistry> = match &self.subagent_registry {
            Some(r) => r.clone(),
            None => Arc::new(crate::subagent::SubAgentRegistry::with_builtins()),
        };

        let spec = registry.get(agent_name).cloned().unwrap_or_else(|| {
            theo_domain::agent_spec::AgentSpec::on_demand(agent_name, instructions)
        });

        let manager = crate::subagent::SubAgentManager::with_registry(
            self.config.clone(),
            self.event_bus.clone(),
            self.project_dir.clone(),
            registry,
        )
        .with_metrics(self.metrics.clone());

        let sub_result = manager
            .spawn_with_spec_text(&spec, instructions, None)
            .await;

        let result_msg = if sub_result.success {
            format!("[Skill '{}' completed] {}", skill_name, sub_result.summary)
        } else {
            format!("[Skill '{}' failed] {}", skill_name, sub_result.summary)
        };

        for file in &sub_result.files_edited {
            if !file.is_empty() {
                self.context_loop_state
                    .record_edit_attempt(file, true, None);
            }
        }

        self.budget_enforcer.record_tokens(sub_result.tokens_used);
        self.metrics.record_delegated_tokens(sub_result.tokens_used);

        messages.push(Message::tool_result(&call.id, "skill", &result_msg));
    }
}
