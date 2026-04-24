//! Session bootstrap: assemble the initial `Vec<Message>` at the start
//! of `execute_with_history`.
//!
//! Extracted from `run_engine/mod.rs` (Fase 4 — REMEDIATION_PLAN T4.2)
//! into a separate `impl AgentRunEngine` block. The assembly stage is a
//! self-contained ~200-LOC sequence of system-prompt composition,
//! memory prefetch, episode replay, git log injection, skill catalog,
//! and history merge — collapsed here into a single async method that
//! returns the fully-populated message list.
//!
//! Moving this out of the main loop is a prerequisite for the further
//! `main_loop.rs` / `dispatch/*` extractions planned in T4.2/T4.3.

use theo_infra_llm::types::Message;

use super::AgentRunEngine;
use crate::run_engine_auto_init::auto_init_project_context;
use crate::skill::SkillRegistry;

impl AgentRunEngine {
    /// Assemble the initial `Vec<Message>` for the LLM call. Runs
    /// side effects (state-machine transitions, auto-init, autodream
    /// spawn, episode counter bumps) before returning.
    ///
    /// Caller is expected to chain this into the main loop setup.
    pub(super) async fn assemble_initial_messages(
        &mut self,
        history: Vec<Message>,
    ) -> Vec<Message> {
        use theo_domain::agent_run::RunState;
        use theo_domain::task::TaskState;

        // Transition to Planning
        self.transition_run(RunState::Planning);

        // Transition task to Running
        let _ = self.task_manager.transition(&self.task_id, TaskState::Ready);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Running);

        // Auto-init: create .theo/theo.md if it doesn't exist (main agent only).
        // Uses static template — instantaneous, no LLM cost. The agent can
        // enrich later. Best-effort: if write fails, continue without
        // project context.
        if !self.config.is_subagent {
            auto_init_project_context(&self.project_dir);
        }

        // Autodream at session start (main agent only).
        if !self.config.is_subagent {
            crate::memory_lifecycle::maybe_spawn_autodream(
                &self.config,
                &self.autodream_attempted,
                &self.project_dir,
                self.run.run_id.as_str(),
            );
        }

        // System prompt: .theo/system-prompt.md replaces default, or use
        // config default. Bootstrap prompt prepended when USER.md is
        // missing/empty.
        let base_prompt = if !self.config.is_subagent {
            crate::project_config::load_system_prompt(&self.project_dir)
                .unwrap_or_else(|| self.config.system_prompt.clone())
        } else {
            self.config.system_prompt.clone()
        };
        let system_prompt = crate::memory_lifecycle::maybe_prepend_bootstrap(
            &self.config,
            &self.project_dir,
            base_prompt,
        );

        let mut messages: Vec<Message> = vec![Message::system(&system_prompt)];

        // Project context: .theo/theo.md prepended as separate system message
        if !self.config.is_subagent
            && let Some(context) =
                crate::project_config::load_project_context(&self.project_dir)
        {
            messages.push(Message::system(format!("## Project Context\n{context}")));
        }

        // GRAPHCTX is available as the `codebase_context` tool — the LLM
        // calls it on-demand. No automatic injection: the LLM decides
        // when it needs code structure context. The graph_context
        // provider is passed to tools via ToolContext.graph_context.

        // Memory injection: prefetch when enabled (sole source), else
        // legacy FileMemoryStore fallback. Dual-injection is prevented
        // by this explicit branch.
        if self.config.memory_enabled {
            let query = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_else(|| "session".into());
            let _ = crate::memory_lifecycle::run_engine_hooks::inject_prefetch(
                &self.config,
                &mut messages,
                &query,
            )
            .await;
        } else {
            crate::memory_lifecycle::run_engine_hooks::inject_legacy_file_memory(
                &self.project_dir,
                &mut messages,
            )
            .await;
        }

        // Feed eligible episode summaries back into context
        // (lifecycle != Archived, TTL not expired, 5% token budget).
        if !self.config.is_subagent {
            let injected =
                crate::memory_lifecycle::run_engine_hooks::inject_episode_history(
                    &self.project_dir,
                    self.config.context_window_tokens,
                    &mut messages,
                );
            self.episodes_injected = self.episodes_injected.saturating_add(injected as u32);
        }

        // Boot sequence: inject progress from previous sessions + recent
        // git activity. Inserted after memories, before skills — so the
        // agent knows where it left off.
        if !self.config.is_subagent {
            self.inject_boot_context(&mut messages).await;
        }

        // Planning injection: if GRAPHCTX is Ready, inject top-5
        // relevant files as system message so the LLM starts with
        // structural orientation. Skip if Building (don't use stale
        // for planning), only use fresh Ready state.
        if !self.config.is_subagent {
            self.inject_planning_context(&mut messages).await;
        }

        // Inject available skills into system context (main agent only).
        // Sub-agents do NOT receive skills — they execute their direct
        // objective. This is Layer 2 of recursive spawning prevention.
        if !self.config.is_subagent {
            inject_skills_summary(&self.project_dir, &mut messages);
        }

        // Inject session history (previous REPL prompts + responses)
        if !history.is_empty() {
            messages.extend(history);
        }

        // Add the task objective as user message
        if let Some(task) = self.task_manager.get(&self.task_id) {
            messages.push(Message::user(&task.objective));
        }

        messages
    }

    async fn inject_boot_context(&mut self, messages: &mut Vec<Message>) {
        let mut boot_parts: Vec<String> = Vec::new();

        // Previous session progress
        if let Some(progress_msg) =
            crate::session_bootstrap::boot_message(&self.project_dir)
        {
            boot_parts.push(progress_msg);
        }

        // Recent git activity (max 20 commits, best-effort).
        // Uses tokio::process to avoid blocking the async worker on a
        // slow/locked git repo. Commit messages are user-controlled so
        // they go through fence_untrusted to neutralize provider
        // control tokens (prompt-injection countermeasure, T1.2).
        if let Ok(output) = tokio::process::Command::new("git")
            .args(["log", "--oneline", "-20"])
            .current_dir(&self.project_dir)
            .output()
            .await
            && output.status.success()
        {
            let log = String::from_utf8_lossy(&output.stdout);
            let log = log.trim();
            if !log.is_empty() {
                let fenced = theo_domain::prompt_sanitizer::fence_untrusted_default(
                    log, "git-log",
                );
                boot_parts.push(format!("Recent git commits:\n{fenced}"));
            }
        }

        if !boot_parts.is_empty() {
            messages.push(Message::system(format!(
                "## Session Boot Context\n{}",
                boot_parts.join("\n\n")
            )));
        }
    }

    async fn inject_planning_context(&mut self, messages: &mut Vec<Message>) {
        let Some(ref provider) = self.graph_context else {
            return;
        };
        if !provider.is_ready() {
            return;
        }
        // Use the task objective (first user message) as query
        let planning_query = messages
            .iter()
            .rev()
            .find(|m| m.role == theo_infra_llm::types::Role::User)
            .and_then(|m| m.content.as_deref())
            .unwrap_or("")
            .chars()
            .take(crate::constants::TOOL_PREVIEW_BYTES)
            .collect::<String>();

        if planning_query.is_empty() {
            return;
        }
        let Ok(ctx) = provider.query_context(&planning_query, 1000).await else {
            return;
        };
        if ctx.blocks.is_empty() {
            return;
        }
        // Record initial context files for the task-derailment sensor.
        for b in ctx.blocks.iter().take(5) {
            self.initial_context_files.insert(b.source_id.clone());
        }
        let file_hints: Vec<String> = ctx
            .blocks
            .iter()
            .take(5)
            .map(|b| format!("- {} (relevance: {:.0}%)", b.source_id, b.score * 100.0))
            .collect();
        messages.push(Message::system(format!(
            "## Suggested Starting Files\nBased on code graph analysis, these areas are most relevant to your task:\n{}\n\nStart here, but verify with read/grep.",
            file_hints.join("\n")
        )));
    }
}

/// Build the skill registry (bundled + project + user) and append a
/// skills-summary system message if any skill triggers are declared.
fn inject_skills_summary(project_dir: &std::path::Path, messages: &mut Vec<Message>) {
    let mut skill_registry = SkillRegistry::new();
    skill_registry.load_bundled();
    let project_skills = project_dir.join(".theo").join("skills");
    if project_skills.exists() {
        skill_registry.load_from_dir(&project_skills);
    }
    // Load global skills only when HOME is set. Previously fell back
    // to /tmp/.config/theo/skills — shared/untrusted path.
    if let Some(user_skills) = theo_domain::user_paths::theo_config_subdir("skills")
        && user_skills.exists()
    {
        skill_registry.load_from_dir(&user_skills);
    }
    let skills_summary = skill_registry.triggers_summary();
    if !skills_summary.is_empty() {
        messages.push(Message::system(format!(
            "## Skills\nYou have specialized skills that you SHOULD invoke when the task matches:\n{skills_summary}\n\nWhen the user's request matches a skill trigger, use the `skill` tool to invoke it."
        )));
    }
}
