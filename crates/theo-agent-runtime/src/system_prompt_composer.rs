//! System prompt composition with feature-guarded sections.
//!
//! Replaces monolithic template strings with a builder where each section is
//! `Option<SectionBody>`. Sections only render when their guard flag is true,
//! avoiding token cost for irrelevant rules (e.g. git workflow in a non-git
//! project, sandbox rules when bash is disabled).
//!
//! Reference: `referencias/gemini-cli/packages/core/src/prompts/promptProvider.ts:138-244`
//!
//! The C2 criterion in `.theo/evolution_criteria.md` requires the base
//! system prompt ≤10k tokens — the composer enforces this via `estimated_tokens`.

use theo_domain::tokens::estimate_tokens;

/// Hard cap on the rendered base system prompt (tokens).
pub const BASE_PROMPT_TOKEN_BUDGET: usize = 10_000;

/// Flags driving which sections render. Each flag corresponds to one section.
#[derive(Debug, Clone, Copy, Default)]
pub struct PromptGuards {
    pub git_repo: bool,
    pub sandbox_enabled: bool,
    pub mcps_registered: bool,
    pub subdir_instructions_loaded: bool,
    pub skills_available: bool,
}

/// Composer for the base system prompt.
///
/// Usage:
/// ```ignore
/// let prompt = SystemPromptComposer::new("You are Theo.")
///     .with_core_mandates("Code in English. Tests mandatory.")
///     .with_git(true, "Use conventional commits.")
///     .with_sandbox(false, "")
///     .render();
/// ```
#[derive(Debug, Default, Clone)]
pub struct SystemPromptComposer {
    preamble: String,
    core_mandates: Option<String>,
    git: Option<String>,
    sandbox: Option<String>,
    mcps: Option<String>,
    subdir: Option<String>,
    skills: Option<String>,
    /// Phase 4 / D2 + D5: heurísticas de delegate_task — quando delegar e
    /// como reagir a falhas. Renderizado quando subagents estão disponíveis.
    delegation: Option<String>,
}

impl SystemPromptComposer {
    pub fn new(preamble: impl Into<String>) -> Self {
        Self {
            preamble: preamble.into(),
            ..Default::default()
        }
    }

    pub fn with_core_mandates(mut self, body: impl Into<String>) -> Self {
        self.core_mandates = Some(body.into());
        self
    }

    pub fn with_git(mut self, enabled: bool, body: impl Into<String>) -> Self {
        if enabled {
            self.git = Some(body.into());
        }
        self
    }

    pub fn with_sandbox(mut self, enabled: bool, body: impl Into<String>) -> Self {
        if enabled {
            self.sandbox = Some(body.into());
        }
        self
    }

    pub fn with_mcps(mut self, enabled: bool, body: impl Into<String>) -> Self {
        if enabled {
            self.mcps = Some(body.into());
        }
        self
    }

    pub fn with_subdir_instructions(mut self, enabled: bool, body: impl Into<String>) -> Self {
        if enabled {
            self.subdir = Some(body.into());
        }
        self
    }

    pub fn with_skills(mut self, enabled: bool, body: impl Into<String>) -> Self {
        if enabled {
            self.skills = Some(body.into());
        }
        self
    }

    /// Phase 4 / D2 + D5: attach delegation heuristics — when to use
    /// `delegate_task` vs do work inline, and how to react when sub-agents fail.
    pub fn with_delegation(mut self, enabled: bool, body: impl Into<String>) -> Self {
        if enabled {
            self.delegation = Some(body.into());
        }
        self
    }

    /// Default delegation heuristics body (D2 + D5 from agents-plan.md v3.0).
    pub fn default_delegation_heuristics() -> &'static str {
        "## When to use delegate_task\n\n\
         DELEGATE when:\n\
         - The task requires exploring MORE than 5 files to understand context.\n\
           → delegate to `explorer` first, then act on findings.\n\
         - The task has independent sub-problems that can run in parallel.\n\
           → use delegate_task with a `parallel` array.\n\
         - The task requires code review or security analysis.\n\
           → delegate to `reviewer` or a custom security agent.\n\
         - The user explicitly asks for parallel work or agent delegation.\n\
         \n\
         DO NOT delegate when:\n\
         - The task is a single file edit or simple question.\n\
         - You already have enough context from previous tool calls.\n\
         - The task requires sequential decisions where each step depends on the previous.\n\
         \n\
         COST AWARENESS:\n\
         - Each sub-agent consumes a full agent loop (iterations + tokens).\n\
         - On-demand agents are limited to 10 iterations and read-only access.\n\
         - Prefer named agents (explorer, implementer, verifier, reviewer) over on-demand.\n\
         - Use parallel delegation only when tasks are truly independent.\n\
         \n\
         ## When a sub-agent fails (D5)\n\n\
         If `delegate_task` returns success=false:\n\
         1. READ the summary to understand WHY it failed.\n\
         2. If timeout: re-delegate with a more focused objective or smaller scope.\n\
         3. If max_iterations: the task may be too complex — break it into smaller sub-tasks.\n\
         4. If error: investigate the error, fix the issue, then re-delegate.\n\
         5. Do NOT blindly retry the same delegation — diagnose first."
    }

    /// Render the composed prompt to a single string.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.preamble);
        self.append_section(&mut out, "Core Mandates", self.core_mandates.as_deref());
        self.append_section(&mut out, "Git Workflow", self.git.as_deref());
        self.append_section(&mut out, "Sandbox Rules", self.sandbox.as_deref());
        self.append_section(&mut out, "MCP Tools", self.mcps.as_deref());
        self.append_section(
            &mut out,
            "Subdir Instructions",
            self.subdir.as_deref(),
        );
        self.append_section(&mut out, "Skills", self.skills.as_deref());
        self.append_section(&mut out, "Delegation", self.delegation.as_deref());
        out
    }

    fn append_section(&self, out: &mut String, title: &str, body: Option<&str>) {
        if let Some(body) = body {
            if body.is_empty() {
                return;
            }
            out.push_str("\n\n## ");
            out.push_str(title);
            out.push('\n');
            out.push_str(body);
        }
    }

    /// Estimated token count of the rendered prompt.
    pub fn estimated_tokens(&self) -> usize {
        estimate_tokens(&self.render())
    }

    /// Whether the rendered prompt fits inside the base budget (C2).
    pub fn fits_budget(&self) -> bool {
        self.estimated_tokens() <= BASE_PROMPT_TOKEN_BUDGET
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_composer_renders_only_preamble() {
        let p = SystemPromptComposer::new("You are Theo.");
        assert_eq!(p.render(), "You are Theo.");
    }

    #[test]
    fn delegation_section_omitted_when_disabled() {
        let p = SystemPromptComposer::new("pre")
            .with_delegation(false, "anything");
        assert!(!p.render().contains("Delegation"));
    }

    #[test]
    fn delegation_section_renders_when_enabled() {
        let p = SystemPromptComposer::new("pre")
            .with_delegation(true, SystemPromptComposer::default_delegation_heuristics());
        let out = p.render();
        assert!(out.contains("Delegation"));
        assert!(out.contains("DELEGATE when:"));
        assert!(out.contains("DO NOT delegate when:"));
        assert!(out.contains("COST AWARENESS"));
        assert!(out.contains("When a sub-agent fails")); // D5
    }

    #[test]
    fn default_delegation_heuristics_mentions_d2_d5() {
        let body = SystemPromptComposer::default_delegation_heuristics();
        // D2 — when to delegate
        assert!(body.contains("DELEGATE when"));
        assert!(body.contains("DO NOT delegate"));
        // D5 — failure handling
        assert!(body.contains("If timeout"));
        assert!(body.contains("If max_iterations"));
        assert!(body.contains("Do NOT blindly retry"));
    }

    #[test]
    fn core_mandates_always_render_when_set() {
        let p = SystemPromptComposer::new("pre")
            .with_core_mandates("rule A");
        let out = p.render();
        assert!(out.contains("Core Mandates"));
        assert!(out.contains("rule A"));
    }

    #[test]
    fn git_section_omitted_when_not_a_git_repo() {
        let p = SystemPromptComposer::new("pre")
            .with_git(false, "use conventional commits");
        let out = p.render();
        assert!(!out.contains("Git Workflow"));
        assert!(!out.contains("conventional commits"));
    }

    #[test]
    fn git_section_present_when_enabled() {
        let p = SystemPromptComposer::new("pre")
            .with_git(true, "use conventional commits");
        let out = p.render();
        assert!(out.contains("Git Workflow"));
        assert!(out.contains("conventional commits"));
    }

    #[test]
    fn sandbox_section_omitted_when_bash_disabled() {
        let p = SystemPromptComposer::new("pre")
            .with_sandbox(false, "bwrap mandatory");
        assert!(!p.render().contains("Sandbox"));
    }

    #[test]
    fn mcps_section_omitted_when_none_registered() {
        let p = SystemPromptComposer::new("pre")
            .with_mcps(false, "list of mcps");
        assert!(!p.render().contains("MCP"));
    }

    #[test]
    fn section_omitted_when_body_empty() {
        let p = SystemPromptComposer::new("pre")
            .with_git(true, "");
        assert!(!p.render().contains("Git Workflow"));
    }

    #[test]
    fn render_is_idempotent() {
        let p = SystemPromptComposer::new("pre")
            .with_core_mandates("rule")
            .with_git(true, "workflow");
        assert_eq!(p.render(), p.render());
    }

    #[test]
    fn minimal_prompt_fits_budget_easily() {
        let p = SystemPromptComposer::new("You are Theo, an AI coding assistant.")
            .with_core_mandates("Write tests first. Typed errors. No unwrap.");
        assert!(p.fits_budget());
        assert!(p.estimated_tokens() < 100);
    }

    #[test]
    fn large_prompt_detects_budget_violation() {
        let big = "x".repeat(BASE_PROMPT_TOKEN_BUDGET * 5);
        let p = SystemPromptComposer::new("pre").with_core_mandates(big);
        assert!(!p.fits_budget());
    }

    #[test]
    fn sections_render_in_stable_order() {
        let p = SystemPromptComposer::new("pre")
            .with_skills(true, "S")
            .with_git(true, "G")
            .with_core_mandates("C");
        let out = p.render();
        let core_idx = out.find("Core Mandates").unwrap();
        let git_idx = out.find("Git Workflow").unwrap();
        let skills_idx = out.find("Skills").unwrap();
        assert!(core_idx < git_idx);
        assert!(git_idx < skills_idx);
    }
}
