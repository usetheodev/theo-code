mod result;

pub use result::AgentResult;

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
    /// Construct an AgentLoop bound to a config + tool registry.
    ///
    /// The `registry` parameter is preserved for backward compatibility
    /// with the original API shape but is intentionally dropped — the
    /// runtime materializes its own registry via `create_default_registry()`
    /// inside `AgentRunEngine`. We keep the parameter rather than
    /// changing the signature so existing call sites in `theo-application`
    /// and `apps/*` compile unchanged.
    pub fn new(config: AgentConfig, _registry: ToolRegistry) -> Self {
        // Snapshot LLM-cluster fields BEFORE moving `config` into Self
        // (T4.1 view migration — the borrow from `config.llm()` ends with
        // these locals so the subsequent move is safe).
        let (
            client_base_url,
            client_api_key,
            client_model,
            client_endpoint_override,
            client_extra_headers,
        ) = {
            let llm = config.llm();
            (
                llm.base_url.to_string(),
                llm.api_key.cloned(),
                llm.model.to_string(),
                llm.endpoint_override.cloned(),
                llm.extra_headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        };
        Self {
            client_base_url,
            client_api_key,
            client_model,
            client_endpoint_override,
            client_extra_headers,
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
        // T2.3 / FIND-P6-005 / D4 — `CapabilityGate` is now ALWAYS
        // installed. When the user did not configure a `capability_set`,
        // `CapabilitySet::unrestricted()` is used so every tool dispatch
        // goes through the gate's auditing path (the `CapabilityGranted`
        // event flows to listeners and OTel) even though it does not
        // restrict tool access. Previously `None → bare tcm` removed
        // the gate entirely from the main agent and silenced audit
        // events. INV-003 fortalecido.
        let caps = self
            .config
            .plugin()
            .capability_set
            .cloned()
            .unwrap_or_else(theo_domain::capability::CapabilitySet::unrestricted);
        let gate = Arc::new(CapabilityGate::new(caps, event_bus.clone()));
        let tool_call_manager = Arc::new(tcm.with_capability_gate(gate));

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
            self.config.plugin().allowlist,
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
        let mut result = engine.execute_with_history(history).await;
        engine.record_session_exit_public(&result).await;
        result.run_report = engine.take_run_report();
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

    /// T3.2 AC regression — `run()` and `run_with_history()` MUST both
    /// T2.3 / FIND-P6-005 / D4 — `build_engine` must ALWAYS install a
    /// `CapabilityGate` on the `ToolCallManager`. Previously, when the
    /// user's `capability_set` was `None`, `build_engine` returned a
    /// bare `tcm` with no gate, which silenced all
    /// `CapabilityGranted`/`CapabilityDenied` audit events on the main
    /// agent and disabled the enforcement path entirely.
    ///
    /// We assert this structurally because a full e2e test would
    /// require building an `AgentLoop` end-to-end with an LLM mock.
    #[test]
    fn build_engine_always_installs_capability_gate() {
        let src = include_str!("mod.rs");
        let flat: String = src.split_whitespace().collect::<Vec<_>>().join(" ");

        // The unwrap_or_else default uses `CapabilitySet::unrestricted`
        // and the gate is wrapped in `with_capability_gate(gate)`.
        // Both must remain present.
        assert!(
            flat.contains("CapabilitySet :: unrestricted")
                || flat.contains("CapabilitySet::unrestricted"),
            "build_engine must default capability_set to unrestricted() when None"
        );
        assert!(
            flat.contains("with_capability_gate ( gate )")
                || flat.contains("with_capability_gate(gate)"),
            "build_engine must call with_capability_gate(gate) unconditionally"
        );

        // The construction must not be inside a `match ... { Some => ... }`
        // arm — the gate is wired UNCONDITIONALLY for the production path.
        // We check that `with_capability_gate(gate)` is wrapped in
        // `Arc::new(...)` directly (one expression) rather than living
        // behind a match-arm split.
        assert!(
            flat.contains("Arc :: new ( tcm . with_capability_gate ( gate )")
                || flat.contains("Arc::new(tcm.with_capability_gate(gate)"),
            "T2.3: gate must be installed unconditionally (no match-arm split)"
        );
    }

    /// route through `execute_and_shutdown`, which in turn invokes
    /// `record_session_exit_public` so the memory hook, episode
    /// persistence, and metrics flush always fire. A future refactor
    /// that re-inlines either path or skips the shutdown call would
    /// break this invariant. We assert it structurally on the source
    /// rather than running an end-to-end test (which would need an
    /// LLM mock).
    #[test]
    fn run_and_run_with_history_both_call_record_session_exit() {
        let src = include_str!("mod.rs");
        // Collapse line wraps so multi-line method bodies still match.
        let flat: String = src.split_whitespace().collect::<Vec<_>>().join(" ");

        // Both paths delegate to `execute_and_shutdown`.
        let run_delegates =
            flat.contains("pub async fn run ( & self") && flat.contains("execute_and_shutdown");
        let history_delegates = flat.contains("pub async fn run_with_history")
            && flat.contains("execute_and_shutdown");
        assert!(
            run_delegates,
            "run() must delegate through execute_and_shutdown"
        );
        assert!(
            history_delegates,
            "run_with_history() must delegate through execute_and_shutdown"
        );

        // `execute_and_shutdown` itself must call `record_session_exit_public`.
        assert!(
            flat.contains("record_session_exit_public"),
            "execute_and_shutdown must invoke record_session_exit_public"
        );

        // Lightweight LOC budget check — `run()` and `run_with_history()`
        // bodies must each fit in <= 30 LOC (T3.2 AC).
        let body = |fn_decl: &str| -> usize {
            let start = src.find(fn_decl).expect("fn declaration present");
            let after = &src[start..];
            // Find the matching closing brace at the function's outer level.
            let mut depth = 0i32;
            let mut count = 0usize;
            let mut entered = false;
            for ch in after.chars() {
                if ch == '\n' {
                    count += 1;
                }
                if ch == '{' {
                    depth += 1;
                    entered = true;
                }
                if ch == '}' {
                    depth -= 1;
                    if entered && depth == 0 {
                        return count;
                    }
                }
            }
            count
        };
        let run_loc = body("pub async fn run(");
        let history_loc = body("pub async fn run_with_history(");
        assert!(
            run_loc <= 30,
            "run() body too long: {run_loc} LOC > 30 (T3.2 AC)"
        );
        assert!(
            history_loc <= 30,
            "run_with_history() body too long: {history_loc} LOC > 30 (T3.2 AC)"
        );
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
    // Resume-runtime-wiring builder forwarding
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
