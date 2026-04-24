use std::path::Path;
use std::sync::Arc;

use theo_domain::session::SessionId;
use theo_domain::task::AgentType;
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

use crate::capability_gate::CapabilityGate;
use crate::config::AgentConfig;
use crate::event_bus::{EventBus, EventListener};
use crate::run_engine::AgentRunEngine;
use crate::task_manager::TaskManager;
use crate::tool_call_manager::ToolCallManager;

/// Result of an agent loop execution.
#[derive(Debug, Clone, Default)]
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    pub files_edited: Vec<String>,
    pub iterations_used: usize,
    /// True when the summary was already displayed via ContentDelta streaming.
    /// The REPL should NOT re-print the summary in this case to avoid duplication.
    /// Only set for text-only responses (no tool calls) where content == summary.
    pub was_streamed: bool,
    /// Total tokens consumed during this run (LLM input + output).
    /// Collected by MetricsCollector, surfaced for display.
    pub tokens_used: u64,
    /// Input (prompt) tokens consumed during this run.
    pub input_tokens: u64,
    /// Output (completion) tokens consumed during this run.
    pub output_tokens: u64,
    /// Total tool calls dispatched during this run.
    pub tool_calls_total: u64,
    /// Tool calls that returned without error.
    pub tool_calls_success: u64,
    /// Total LLM API calls during this run.
    pub llm_calls: u64,
    /// Total LLM retries triggered during this run.
    pub retries: u64,
    /// Wall-clock duration of the run, milliseconds. Filled by the caller.
    pub duration_ms: u64,
    /// Phase 3: name of the agent that produced this result (empty string
    /// for top-level / pre-refactor paths — backward compat).
    #[doc(hidden)]
    pub agent_name: String,
    /// Phase 3: optional raw context string passed to the sub-agent (if any).
    #[doc(hidden)]
    pub context_used: Option<String>,
    /// Phase 7: structured output extracted from `summary` per the spec's
    /// `output_format`. `None` if no schema declared, parse failed in
    /// best_effort mode, or output is plain text.
    #[doc(hidden)]
    pub structured: Option<serde_json::Value>,
    /// Phase 6: true when the run terminated via cooperative cancellation
    /// (parent cancelled, root token, or per-agent token). Distinct from
    /// `success: false` which covers errors / timeouts.
    #[doc(hidden)]
    pub cancelled: bool,
    /// Phase 11: path of the isolated worktree when isolation=worktree.
    /// `None` when the sub-agent ran in shared CWD.
    #[doc(hidden)]
    pub worktree_path: Option<std::path::PathBuf>,
    /// Phase 59 (headless-error-classification-plan): typed reason for
    /// the outcome. `None` only on legacy paths that haven't been
    /// migrated. Headless v3 schema emits this field; downstream
    /// statistical comparators use it to separate real agent failures
    /// from infra failures (rate-limit, auth, sandbox).
    ///
    /// Invariant (validated by tests): `success == true ⇔ class ==
    /// Some(ErrorClass::Solved)`.
    pub error_class: Option<theo_domain::error_class::ErrorClass>,
}

impl AgentResult {
    /// Build an `AgentResult` from an engine's current metrics snapshot.
    ///
    /// Replaces ~5 duplicated inline-struct literals that scattered the
    /// 12 metric fields across `run_engine.rs` return paths (REVIEW §2 /
    /// T3.1). Callers still set `success`, `summary`, `was_streamed`,
    /// `error_class`, and `iterations_used` — everything else comes from
    /// the engine state.
    pub fn from_engine_state(
        engine: &crate::run_engine::AgentRunEngine,
        success: bool,
        summary: String,
        was_streamed: bool,
        error_class: theo_domain::error_class::ErrorClass,
    ) -> Self {
        let m = engine.metrics();
        let (files_edited, iteration) = engine.run_result_context();
        Self {
            success,
            summary,
            was_streamed,
            files_edited,
            iterations_used: iteration,
            tokens_used: m.total_tokens_used,
            input_tokens: m.total_input_tokens,
            output_tokens: m.total_output_tokens,
            tool_calls_total: m.total_tool_calls,
            tool_calls_success: m.successful_tool_calls,
            llm_calls: m.total_llm_calls,
            retries: m.total_retries,
            duration_ms: 0,
            error_class: Some(error_class),
            ..Default::default()
        }
    }
}

/// T5.2 — bundle of 11 sub-agent integrations previously exposed as 11
/// separate `with_subagent_*` builders on `AgentLoop`.
///
/// Callers can populate this once and pass via
/// [`AgentLoop::with_subagent_integrations`] instead of chaining N
/// builder calls. The individual `with_subagent_*` methods remain for
/// backward compatibility but are documented as deprecated — new code
/// should use this struct.
#[derive(Default, Clone)]
pub struct SubAgentIntegrations {
    pub registry: Option<Arc<crate::subagent::SubAgentRegistry>>,
    pub run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    pub hooks: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    pub cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    pub checkpoint: Option<Arc<crate::checkpoint::CheckpointManager>>,
    pub worktree: Option<Arc<theo_isolation::WorktreeProvider>>,
    pub mcp: Option<Arc<theo_infra_mcp::McpRegistry>>,
    pub mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    pub handoff_guardrails: Option<Arc<crate::handoff_guardrail::GuardrailChain>>,
    pub reloadable: Option<crate::subagent::ReloadableRegistry>,
    pub resume_context: Option<Arc<crate::subagent::resume::ResumeContext>>,
}

/// The main agent loop that orchestrates LLM ↔ tool execution.
///
/// This is now a thin facade over `AgentRunEngine`. All execution logic
/// lives in `run_engine.rs`.
pub struct AgentLoop {
    client_base_url: String,
    client_api_key: Option<String>,
    client_model: String,
    client_endpoint_override: Option<String>,
    client_extra_headers: Vec<(String, String)>,
    #[allow(dead_code)] // Stored for backward compat; runtime uses create_default_registry()
    registry: ToolRegistry,
    config: AgentConfig,
    listeners: Vec<Arc<dyn EventListener>>,
    graph_context: Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>>,
    // Sub-agent integrations forwarded to AgentRunEngine. Prefer
    // `with_subagent_integrations(SubAgentIntegrations)` to set these
    // in bulk — the per-field builders below remain for backward
    // compatibility.
    subagent_registry: Option<Arc<crate::subagent::SubAgentRegistry>>,
    subagent_run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    subagent_hooks: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    subagent_cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    subagent_checkpoint: Option<Arc<crate::checkpoint::CheckpointManager>>,
    subagent_worktree: Option<Arc<theo_isolation::WorktreeProvider>>,
    subagent_mcp: Option<Arc<theo_infra_mcp::McpRegistry>>,
    /// MCP discovery cache forwarded to AgentRunEngine.
    subagent_mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    /// Handoff guardrail chain forwarded to AgentRunEngine.
    subagent_handoff_guardrails:
        Option<Arc<crate::handoff_guardrail::GuardrailChain>>,
    subagent_reloadable: Option<crate::subagent::ReloadableRegistry>,
    /// Resume replay context: when present, dispatch consults the
    /// context BEFORE invoking each tool. Already-completed call_ids
    /// replay their cached `Message::tool_result` instead of
    /// re-executing the tool. `None` means default dispatch.
    resume_context: Option<Arc<crate::subagent::resume::ResumeContext>>,
}

impl AgentLoop {
    pub fn new(config: AgentConfig, registry: ToolRegistry) -> Self {
        Self {
            client_base_url: config.base_url.clone(),
            client_api_key: config.api_key.clone(),
            client_model: config.model.clone(),
            client_endpoint_override: config.endpoint_override.clone(),
            client_extra_headers: config
                .extra_headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            registry,
            config,
            listeners: Vec::new(),
            graph_context: None,
            subagent_registry: None,
            subagent_run_store: None,
            subagent_hooks: None,
            subagent_cancellation: None,
            subagent_checkpoint: None,
            subagent_worktree: None,
            subagent_mcp: None,
            subagent_mcp_discovery: None,
            subagent_handoff_guardrails: None,
            subagent_reloadable: None,
            resume_context: None,
        }
    }

    /// Attach a native domain-event listener to the loop.
    #[must_use]
    pub fn with_event_listener(mut self, listener: Arc<dyn EventListener>) -> Self {
        self.listeners.push(listener);
        self
    }

    /// Set the graph context provider for code intelligence injection.
    pub fn with_graph_context(
        mut self,
        provider: Arc<dyn theo_domain::graph_context::GraphContextProvider>,
    ) -> Self {
        self.graph_context = Some(provider);
        self
    }

    /// Inject a SubAgentRegistry into AgentLoop → forwarded to AgentRunEngine.
    pub fn with_subagent_registry(mut self, r: Arc<crate::subagent::SubAgentRegistry>) -> Self {
        self.subagent_registry = Some(r);
        self
    }

    pub fn with_subagent_run_store(
        mut self,
        s: Arc<crate::subagent_runs::FileSubagentRunStore>,
    ) -> Self {
        self.subagent_run_store = Some(s);
        self
    }

    pub fn with_subagent_hooks(mut self, h: Arc<crate::lifecycle_hooks::HookManager>) -> Self {
        self.subagent_hooks = Some(h);
        self
    }

    pub fn with_subagent_cancellation(
        mut self,
        c: Arc<crate::cancellation::CancellationTree>,
    ) -> Self {
        self.subagent_cancellation = Some(c);
        self
    }

    pub fn with_subagent_checkpoint(
        mut self,
        m: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.subagent_checkpoint = Some(m);
        self
    }

    pub fn with_subagent_worktree(mut self, w: Arc<theo_isolation::WorktreeProvider>) -> Self {
        self.subagent_worktree = Some(w);
        self
    }

    pub fn with_subagent_mcp(mut self, m: Arc<theo_infra_mcp::McpRegistry>) -> Self {
        self.subagent_mcp = Some(m);
        self
    }

    /// Inject the MCP discovery cache.
    pub fn with_subagent_mcp_discovery(
        mut self,
        cache: Arc<theo_infra_mcp::DiscoveryCache>,
    ) -> Self {
        self.subagent_mcp_discovery = Some(cache);
        self
    }

    /// Inject the handoff guardrail chain.
    pub fn with_subagent_handoff_guardrails(
        mut self,
        chain: Arc<crate::handoff_guardrail::GuardrailChain>,
    ) -> Self {
        self.subagent_handoff_guardrails = Some(chain);
        self
    }

    pub fn with_subagent_reloadable(
        mut self,
        r: crate::subagent::ReloadableRegistry,
    ) -> Self {
        self.subagent_reloadable = Some(r);
        self
    }

    /// Enable replay-mode dispatch. When set, each tool call is
    /// consulted against the context's `executed_tool_calls` set
    /// BEFORE dispatching. Hits get the cached `Message::tool_result`
    /// from the event log. Misses dispatch normally. The Resumer
    /// constructs the context and invokes this builder per resume.
    pub fn with_resume_context(
        mut self,
        ctx: Arc<crate::subagent::resume::ResumeContext>,
    ) -> Self {
        self.resume_context = Some(ctx);
        self
    }

    /// T5.2 — apply an entire bundle of sub-agent integrations in one
    /// call instead of chaining 11 individual `with_subagent_*` builders.
    /// Any field left `None` leaves the existing state untouched.
    #[must_use]
    pub fn with_subagent_integrations(mut self, integrations: SubAgentIntegrations) -> Self {
        if let Some(v) = integrations.registry {
            self.subagent_registry = Some(v);
        }
        if let Some(v) = integrations.run_store {
            self.subagent_run_store = Some(v);
        }
        if let Some(v) = integrations.hooks {
            self.subagent_hooks = Some(v);
        }
        if let Some(v) = integrations.cancellation {
            self.subagent_cancellation = Some(v);
        }
        if let Some(v) = integrations.checkpoint {
            self.subagent_checkpoint = Some(v);
        }
        if let Some(v) = integrations.worktree {
            self.subagent_worktree = Some(v);
        }
        if let Some(v) = integrations.mcp {
            self.subagent_mcp = Some(v);
        }
        if let Some(v) = integrations.mcp_discovery {
            self.subagent_mcp_discovery = Some(v);
        }
        if let Some(v) = integrations.handoff_guardrails {
            self.subagent_handoff_guardrails = Some(v);
        }
        if let Some(v) = integrations.reloadable {
            self.subagent_reloadable = Some(v);
        }
        if let Some(v) = integrations.resume_context {
            self.resume_context = Some(v);
        }
        self
    }

    /// Forward all subagent integrations to a freshly-built AgentRunEngine.
    fn forward_subagent_integrations(&self, mut engine: AgentRunEngine) -> AgentRunEngine {
        if let Some(r) = &self.subagent_registry {
            engine = engine.with_subagent_registry(r.clone());
        }
        if let Some(s) = &self.subagent_run_store {
            engine = engine.with_subagent_run_store(s.clone());
        }
        if let Some(h) = &self.subagent_hooks {
            engine = engine.with_subagent_hooks(h.clone());
        }
        if let Some(c) = &self.subagent_cancellation {
            engine = engine.with_subagent_cancellation(c.clone());
        }
        if let Some(cm) = &self.subagent_checkpoint {
            engine = engine.with_subagent_checkpoint(cm.clone());
        }
        if let Some(w) = &self.subagent_worktree {
            engine = engine.with_subagent_worktree(w.clone());
        }
        if let Some(m) = &self.subagent_mcp {
            engine = engine.with_subagent_mcp(m.clone());
        }
        if let Some(d) = &self.subagent_mcp_discovery {
            engine = engine.with_subagent_mcp_discovery(d.clone());
        }
        if let Some(g) = &self.subagent_handoff_guardrails {
            engine = engine.with_subagent_handoff_guardrails(g.clone());
        }
        if let Some(r) = &self.subagent_reloadable {
            engine = engine.with_subagent_reloadable(r.clone());
        }
        if let Some(rc) = &self.resume_context {
            engine = engine.with_resume_context(rc.clone());
        }
        engine
    }

    /// Run the agent loop on a task.
    ///
    /// Delegates entirely to `AgentRunEngine::execute()`.
    pub async fn run(&self, task: &str, project_dir: &Path) -> AgentResult {
        let engine = self.build_engine(task, project_dir, None);
        self.execute_and_shutdown(engine, Vec::new()).await
    }

    /// Run with session history and external EventBus.
    ///
    /// The caller provides an EventBus with listeners already subscribed
    /// (e.g., CliRenderer for real-time output).
    pub async fn run_with_history(
        &self,
        task: &str,
        project_dir: &Path,
        history: Vec<theo_infra_llm::types::Message>,
        external_bus: Option<Arc<EventBus>>,
    ) -> AgentResult {
        let engine = self.build_engine(task, project_dir, external_bus);
        self.execute_and_shutdown(engine, history).await
    }

    /// Build an `AgentRunEngine` wired with task manager, LLM client, tool
    /// registry, graph context, and all sub-agent integrations. Extracted
    /// from `run()` / `run_with_history()` to eliminate ~80 LOC of
    /// duplicated setup (T3.2 / REVIEW §2 DRY).
    fn build_engine(
        &self,
        task: &str,
        project_dir: &Path,
        external_bus: Option<Arc<EventBus>>,
    ) -> AgentRunEngine {
        let event_bus = external_bus.unwrap_or_else(|| Arc::new(EventBus::new()));
        self.attach_listeners(&event_bus);

        let task_manager = Arc::new(TaskManager::new(event_bus.clone()));
        let tcm = ToolCallManager::new(event_bus.clone());
        let tool_call_manager = Arc::new(match &self.config.capability_set {
            Some(caps) => {
                let gate = Arc::new(CapabilityGate::new(caps.clone(), event_bus.clone()));
                tcm.with_capability_gate(gate)
            }
            None => tcm,
        });

        let task_id = task_manager.create_task(
            SessionId::new("agent"),
            AgentType::Coder,
            task.to_string(),
        );

        let client = self.build_llm_client();
        let registry = Arc::new(self.build_registry(project_dir, Some(&event_bus)));

        let mut engine = AgentRunEngine::new(
            task_id,
            task_manager,
            tool_call_manager,
            event_bus,
            client,
            registry,
            self.config.clone(),
            project_dir.to_path_buf(),
        );
        if let Some(ref gc) = self.graph_context {
            engine = engine.with_graph_context(gc.clone());
        }
        self.forward_subagent_integrations(engine)
    }

    fn build_llm_client(&self) -> LlmClient {
        let mut client = LlmClient::new(
            &self.client_base_url,
            self.client_api_key.clone(),
            &self.client_model,
        );
        if let Some(ref endpoint) = self.client_endpoint_override {
            client = client.with_endpoint(endpoint);
        }
        for (k, v) in &self.client_extra_headers {
            client = client.with_header(k, v);
        }
        client
    }

    fn build_registry(
        &self,
        project_dir: &Path,
        event_bus: Option<&Arc<EventBus>>,
    ) -> theo_tooling::registry::ToolRegistry {
        let mut registry = theo_tooling::registry::create_default_registry();
        load_plugin_tools(
            &mut registry,
            project_dir,
            self.config.plugin_allowlist.as_ref(),
            event_bus,
        );
        registry
    }

    /// Runs `execute_with_history` then `record_session_exit`. Both
    /// `run()` and `run_with_history()` share this path so the memory
    /// hook + episode persistence + metrics flush fire unconditionally.
    async fn execute_and_shutdown(
        &self,
        mut engine: AgentRunEngine,
        history: Vec<theo_infra_llm::types::Message>,
    ) -> AgentResult {
        let result = engine.execute_with_history(history).await;
        engine.record_session_exit_public(&result).await;
        result
    }

    fn attach_listeners(&self, event_bus: &Arc<EventBus>) {
        for listener in &self.listeners {
            event_bus.subscribe(listener.clone());
        }
    }
}

/// Load plugin tools from .theo/plugins/ into the registry.
/// Honors the optional hash allowlist and emits `PluginLoaded` events
/// when a bus is attached (T1.3).
fn load_plugin_tools(
    registry: &mut theo_tooling::registry::ToolRegistry,
    project_dir: &Path,
    allowlist: Option<&std::collections::BTreeSet<String>>,
    event_bus: Option<&Arc<EventBus>>,
) {
    let plugins = crate::plugin::load_plugins_with_policy(project_dir, allowlist, event_bus);
    let mut tool_specs = Vec::new();
    for plugin in &plugins {
        for (spec, script_path) in &plugin.tool_scripts {
            let params: Vec<theo_domain::tool::ToolParam> = spec
                .params
                .iter()
                .map(|p| theo_domain::tool::ToolParam {
                    name: p.name.clone(),
                    param_type: p.param_type.clone(),
                    description: p.description.clone(),
                    required: p.required,
                })
                .collect();
            tool_specs.push((
                spec.name.clone(),
                spec.description.clone(),
                script_path.clone(),
                params,
            ));
        }
    }
    if !tool_specs.is_empty() {
        theo_tooling::registry::register_plugin_tools(registry, tool_specs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_result_default() {
        let result = AgentResult {
            success: false,
            summary: "test".to_string(),
            ..Default::default()
        };
        assert!(!result.success);
    }

    #[test]
    fn test_agent_loop_new_preserves_constructor_fields() {
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let agent_loop = AgentLoop::new(config.clone(), registry);

        // Verify constructor propagates config correctly
        assert_eq!(agent_loop.client_model, config.model);
        assert!(
            agent_loop.graph_context.is_none(),
            "graph_context should be None by default"
        );
        assert!(agent_loop.listeners.is_empty());
    }

    #[test]
    fn test_with_event_listener_registers_listener() {
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let loop_ = AgentLoop::new(config, registry)
            .with_event_listener(Arc::new(crate::event_bus::NullEventListener));
        assert_eq!(loop_.listeners.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Phase 30 (resume-runtime-wiring) — gap #3 builder forwarding
    // -----------------------------------------------------------------------

    mod with_resume_context {
        use super::*;
        use crate::subagent::resume::{ResumeContext, WorktreeStrategy};
        use std::collections::{BTreeMap, BTreeSet};
        use theo_domain::agent_spec::AgentSpec;

        fn dummy_ctx() -> Arc<ResumeContext> {
            let mut executed = BTreeSet::new();
            executed.insert("c-known".to_string());
            let mut cached = BTreeMap::new();
            cached.insert(
                "c-known".to_string(),
                theo_infra_llm::types::Message::tool_result("c-known", "write_file", "ok"),
            );
            Arc::new(ResumeContext {
                spec: AgentSpec::on_demand("a", "b"),
                start_iteration: 1,
                history: vec![],
                prior_tokens_used: 0,
                checkpoint_before: None,
                executed_tool_calls: executed,
                executed_tool_results: cached,
                worktree_strategy: WorktreeStrategy::None,
            })
        }

        #[test]
        fn agent_loop_without_resume_context_dispatches_normally() {
            // D5 backward compat — default AgentLoop has resume_context = None.
            // Existing 1000+ tests expect this default path to be untouched.
            let config = AgentConfig::default();
            let registry = theo_tooling::registry::create_default_registry();
            let agent = AgentLoop::new(config, registry);
            assert!(
                agent.resume_context.is_none(),
                "default AgentLoop must NOT have resume_context attached"
            );
        }

        #[test]
        fn agent_loop_with_resume_context_attaches_via_builder() {
            let config = AgentConfig::default();
            let registry = theo_tooling::registry::create_default_registry();
            let ctx = dummy_ctx();
            let agent = AgentLoop::new(config, registry).with_resume_context(ctx.clone());

            let attached = agent
                .resume_context
                .as_ref()
                .expect("resume_context must be attached");
            assert!(attached.should_skip_tool_call("c-known"));
        }

        #[test]
        fn agent_loop_with_resume_context_skips_replayed_call_id() {
            // Predicate parity: AgentLoop's stored ResumeContext must answer
            // identically to the contract used inside RunEngine dispatch.
            let config = AgentConfig::default();
            let registry = theo_tooling::registry::create_default_registry();
            let ctx = dummy_ctx();
            let agent = AgentLoop::new(config, registry).with_resume_context(ctx);

            let rc = agent.resume_context.as_ref().unwrap();
            // Replayed call_id → both legs of the dispatch guard fire
            assert!(rc.should_skip_tool_call("c-known"));
            assert!(rc.cached_tool_result("c-known").is_some());
        }

        #[test]
        fn agent_loop_with_resume_context_dispatches_unknown_call_id() {
            // Predicate parity for the "new tool call after resume" path.
            let config = AgentConfig::default();
            let registry = theo_tooling::registry::create_default_registry();
            let ctx = dummy_ctx();
            let agent = AgentLoop::new(config, registry).with_resume_context(ctx);

            let rc = agent.resume_context.as_ref().unwrap();
            // Unknown call_id → no replay, dispatcher must run normally.
            assert!(!rc.should_skip_tool_call("brand-new-call"));
            assert!(rc.cached_tool_result("brand-new-call").is_none());
        }
    }
}
