//! Sibling test body of `mod.rs` (T3.x of god-files-2026-07-23-plan.md).

#![allow(unused_imports)]

use super::*;

    use super::*;
    use super::prompts::default_system_prompt;

    // -----------------------------------------------------------------
    // T4.3 / find_p6_009 — `api_key` must NEVER appear in Debug output.
    // -----------------------------------------------------------------

    #[test]
    fn t43_debug_redacts_api_key_when_present() {
        let mut cfg = AgentConfig::default();
        cfg.llm.api_key = Some("sk-ant-real-secret-do-not-leak".into());
        let dbg = format!("{:?}", cfg);
        assert!(
            !dbg.contains("sk-ant-real-secret-do-not-leak"),
            "raw api_key value leaked into Debug output: {dbg}"
        );
        assert!(
            dbg.contains("[REDACTED]"),
            "expected `[REDACTED]` marker in Debug output: {dbg}"
        );
    }

    #[test]
    fn t43_debug_shows_none_when_api_key_absent() {
        let cfg = AgentConfig::default();
        let dbg = format!("{:?}", cfg);
        // `api_key: None` should still be visible — presence/absence
        // is a useful diagnostic signal.
        assert!(
            dbg.contains("api_key: None"),
            "expected `api_key: None` in Debug output: {dbg}"
        );
    }

    #[test]
    fn t43_debug_pretty_print_does_not_leak_api_key() {
        let mut cfg = AgentConfig::default();
        cfg.llm.api_key = Some("sk-ant-pretty-print-secret".into());
        let pretty = format!("{:#?}", cfg);
        assert!(
            !pretty.contains("sk-ant-pretty-print-secret"),
            "raw api_key value leaked into Debug pretty output: {pretty}"
        );
    }

    /// T4.1 AC literal: each sub-config exposes ≤ 10 fields. After
    /// T3.2 PR1-PR8 the borrowed views are gone and the owned
    /// sub-config structs in `mod.rs` are the source of truth. The AC
    /// stays satisfied as long as no future PR overgrows a sub-config.
    #[test]
    fn each_sub_config_has_at_most_10_fields() {
        let src = include_str!("mod.rs");

        fn count_struct_fields(src: &str, struct_name: &str) -> usize {
            let needle = format!("pub struct {} {{", struct_name);
            let start = src
                .find(&needle)
                .unwrap_or_else(|| panic!("struct {struct_name} not found"));
            let body_start = src[start..]
                .find('{')
                .expect("struct body missing")
                + start;
            let body_end = src[body_start..]
                .find("\n}")
                .expect("struct close missing")
                + body_start;
            src[body_start..body_end]
                .lines()
                .filter(|l| l.trim_start().starts_with("pub "))
                .count()
        }

        for cfg in [
            "LlmConfig",
            "LoopConfig",
            "ContextConfig",
            "MemoryConfig",
            "EvolutionConfig",
            "RoutingConfig",
            "PluginConfig",
        ] {
            let n = count_struct_fields(src, cfg);
            assert!(n <= 10, "{cfg} has {n} fields — T4.1 AC requires <=10");
            assert!(n >= 1, "{cfg} should expose at least one field");
        }
    }

    #[test]
    fn test_default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.loop_cfg.max_iterations, 200);
        assert_eq!(config.llm.temperature, 0.1);
        assert_eq!(config.context.context_loop_interval, 5);
        assert!(config.llm.endpoint_override.is_none());
        assert!(config.llm.extra_headers.is_empty());
    }

    #[test]
    fn is_subagent_false_by_default() {
        let config = AgentConfig::default();
        assert!(
            !config.loop_cfg.is_subagent,
            "main agents must NOT be marked as sub-agents"
        );
    }

    #[test]
    fn agent_mode_default_is_agent() {
        assert_eq!(AgentMode::default(), AgentMode::Agent);
    }

    #[test]
    fn agent_mode_from_str_parses_all_modes() {
        assert_eq!(AgentMode::from_str("agent"), Some(AgentMode::Agent));
        assert_eq!(AgentMode::from_str("plan"), Some(AgentMode::Plan));
        assert_eq!(AgentMode::from_str("ask"), Some(AgentMode::Ask));
        assert_eq!(AgentMode::from_str("PLAN"), Some(AgentMode::Plan));
        assert_eq!(AgentMode::from_str("invalid"), None);
    }

    #[test]
    fn system_prompts_are_distinct_per_mode() {
        let agent = system_prompt_for_mode(AgentMode::Agent);
        let plan = system_prompt_for_mode(AgentMode::Plan);
        let ask = system_prompt_for_mode(AgentMode::Ask);
        assert_ne!(agent, plan);
        assert_ne!(agent, ask);
        assert_ne!(plan, ask);
    }

    #[test]
    fn plan_mode_prompt_requires_visible_text() {
        let prompt = system_prompt_for_mode(AgentMode::Plan);
        assert!(prompt.contains("PLAN MODE"), "missing mode header");
        assert!(
            prompt.contains("visible markdown text"),
            "must instruct the model to write visible text"
        );
        assert!(
            prompt.contains("`think` tool"),
            "must explicitly forbid the think tool"
        );
        assert!(prompt.contains(".theo/plans/"), "missing plan output path");
        assert!(prompt.contains("Tasks"), "missing tasks section");
        assert!(prompt.contains("Risks"), "missing risks section");
    }

    #[test]
    fn ask_mode_prompt_contains_ask_instructions() {
        let prompt = system_prompt_for_mode(AgentMode::Ask);
        assert!(prompt.contains("MODE: ASK"));
        // SOTA prompt rewrite: original literal "clarifying questions" was
        // replaced with the semantically equivalent "questions to clarify
        // requirements". Lock the SEMANTIC contract: the prompt instructs
        // the model to ASK QUESTIONS for clarification.
        assert!(
            prompt.contains("clarify"),
            "ask-mode prompt must instruct the model to clarify"
        );
        assert!(
            prompt.contains("questions"),
            "ask-mode prompt must instruct the model to ask questions"
        );
        assert!(prompt.contains("Do NOT use edit"));
    }

    #[test]
    fn agent_mode_prompt_is_default() {
        let prompt = system_prompt_for_mode(AgentMode::Agent);
        assert_eq!(prompt, AgentConfig::default().context.system_prompt);
    }

    #[test]
    fn default_prompt_contains_harness_engineering_clauses() {
        // SOTA prompt rewrite: the original 4 HE clauses (Clean state
        // contract, Generic tools, Environment legibility, Code
        // intelligence) were replaced with the more comprehensive SOTA
        // structure synthesized from Codex/Claude/Gemini. The CONCEPTS
        // are preserved — this test now locks the semantic contract.
        let prompt = default_system_prompt();

        // Identity: the agent knows it operates inside theo's harness
        assert!(
            prompt.contains("Theo Code") || prompt.contains("Theo agentic harness"),
            "missing harness identity"
        );

        // Clean state contract → verification before done
        assert!(
            prompt.contains("VERIFY") && prompt.contains("done"),
            "missing verification-before-done invariant"
        );

        // Generic tools → tool catalog mentions the core surface
        for tool in &["read", "write", "edit", "bash", "grep", "glob"] {
            assert!(
                prompt.contains(tool),
                "tool catalog missing core tool: {tool}"
            );
        }

        // Environment legibility → memory + persistent state mentioned
        assert!(
            prompt.contains("memory"),
            "missing memory/persistence mention"
        );

        // Code intelligence → codebase_context mentioned
        assert!(
            prompt.contains("codebase_context"),
            "missing codebase_context mention"
        );

        // SOTA invariants added by the rewrite
        assert!(
            prompt.contains("EXECUTE") || prompt.contains("execute"),
            "missing execution emphasis (the SOTA fix for tests_disagree)"
        );
        assert!(
            prompt.contains("git reset --hard") || prompt.contains("force"),
            "missing git safety absolutes"
        );
    }

    #[test]
    fn default_prompt_within_token_budget() {
        // SOTA prompt budget: 3500 tokens max. We approximate at 4 chars
        // per token (conservative for English+code). 3500 tokens ≈ 14000
        // chars. Tighter budget than the previous 2000-token estimate.
        let prompt = default_system_prompt();
        let approx_tokens = prompt.len() / 4;
        assert!(
            approx_tokens <= 3500,
            "default prompt exceeds 3500-token budget: ~{approx_tokens} tokens ({} chars)",
            prompt.len()
        );
    }

    #[test]
    fn default_prompt_mentions_sota_doctrines() {
        // SOTA doctrines synthesized from frontier scaffolds (Codex 5.4,
        // Claude Code 2.1, Gemini CLI). Each is a behavior we know
        // correlates with high pass rates.
        let p = default_system_prompt();
        // Persist until verified (Codex+Gemini)
        assert!(
            p.contains("Persist") || p.contains("persist"),
            "missing persistence doctrine"
        );
        // Action bias — implement, don't propose (Codex)
        assert!(
            p.contains("Never claim success") || p.contains("never propose"),
            "missing action-bias doctrine"
        );
        // Empirical bug reproduction (Gemini)
        assert!(
            p.contains("reproduce") || p.contains("repro"),
            "missing empirical-reproduction doctrine"
        );
        // No over-engineering (Claude)
        assert!(
            p.contains("over-engineer") || p.contains("Don't add"),
            "missing no-over-engineering doctrine"
        );
        // Parallelize independent tools (Codex+Claude)
        assert!(
            p.contains("batch") && p.contains("parallel"),
            "missing parallelization doctrine"
        );
    }

    #[test]
    fn tool_execution_mode_default_is_sequential() {
        assert_eq!(ToolExecutionMode::default(), ToolExecutionMode::Sequential);
    }

    #[test]
    fn tool_execution_mode_from_str_parses_all_modes() {
        assert_eq!(
            ToolExecutionMode::from_str("sequential"),
            Some(ToolExecutionMode::Sequential)
        );
        assert_eq!(
            ToolExecutionMode::from_str("parallel"),
            Some(ToolExecutionMode::Parallel)
        );
        assert_eq!(
            ToolExecutionMode::from_str("PARALLEL"),
            Some(ToolExecutionMode::Parallel)
        );
        assert_eq!(ToolExecutionMode::from_str("invalid"), None);
    }

    #[test]
    fn tool_execution_mode_display() {
        assert_eq!(ToolExecutionMode::Sequential.to_string(), "sequential");
        assert_eq!(ToolExecutionMode::Parallel.to_string(), "parallel");
    }

    #[test]
    fn agent_config_default_uses_sequential_tool_execution() {
        let config = AgentConfig::default();
        assert_eq!(
            config.loop_cfg.tool_execution_mode,
            ToolExecutionMode::Sequential,
            "default config must use sequential tool execution for backward compatibility"
        );
    }

    #[test]
    fn he_clauses_survive_all_modes() {
        // SOTA prompt rewrite: original tested for legacy literal "##
        // Harness Context" + "Clean state contract" headers. The new
        // prompt expresses these CONCEPTS differently. Lock the SEMANTIC
        // contract: every mode mentions the harness identity AND the
        // verification-before-done invariant.
        for mode in [AgentMode::Agent, AgentMode::Plan, AgentMode::Ask] {
            let prompt = system_prompt_for_mode(mode);
            assert!(
                prompt.contains("Theo") || prompt.contains("harness"),
                "harness identity missing in {:?} mode",
                mode
            );
            assert!(
                prompt.contains("done") || prompt.contains("Done"),
                "done-tool contract missing in {:?} mode",
                mode
            );
        }
    }
