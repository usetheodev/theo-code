//! OpenTelemetry GenAI semantic conventions for per-agent observability.
//!
//! Track D — 
//!
//! Implements the attribute names from the official OTel GenAI semantic
//! conventions (2025) WITHOUT pulling in the full `opentelemetry` crate
//! (that would add ~3MB compile time + complex feature flags).
//!
//! Instead, we provide:
//! - Constants for the OTel GenAI attribute keys (`gen_ai.system`,
//!   `gen_ai.agent.id`, `gen_ai.usage.input_tokens`, etc.)
//! - `AgentRunSpan` builder that returns a `BTreeMap<String, Value>` —
//!   directly usable as a JSON payload for `DomainEvent` or as input to
//!   any OTel-compatible exporter
//! - `MetricsByAgent` struct for per-agent metric aggregation
//!
//! Reference:
//! - https://github.com/open-telemetry/semantic-conventions/tree/main/docs/gen-ai
//! - Archon Pino logging convention `{domain}.{action}_{state}`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use theo_domain::agent_spec::AgentSpec;

// ---------------------------------------------------------------------------
// Attribute keys — match OTel GenAI semantic conventions verbatim
// ---------------------------------------------------------------------------

pub const ATTR_SYSTEM: &str = "gen_ai.system";
pub const ATTR_REQUEST_MODEL: &str = "gen_ai.request.model";
pub const ATTR_RESPONSE_MODEL: &str = "gen_ai.response.model";
pub const ATTR_OPERATION_NAME: &str = "gen_ai.operation.name";
pub const ATTR_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
pub const ATTR_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
pub const ATTR_USAGE_TOTAL_TOKENS: &str = "gen_ai.usage.total_tokens";
pub const ATTR_AGENT_ID: &str = "gen_ai.agent.id";
pub const ATTR_AGENT_NAME: &str = "gen_ai.agent.name";

// Theo-specific attributes (namespaced)
pub const ATTR_THEO_AGENT_SOURCE: &str = "theo.agent.source";
pub const ATTR_THEO_AGENT_BUILTIN: &str = "theo.agent.builtin";
pub const ATTR_THEO_DURATION_MS: &str = "theo.run.duration_ms";
pub const ATTR_THEO_ITERATIONS: &str = "theo.run.iterations_used";
pub const ATTR_THEO_LLM_CALLS: &str = "theo.run.llm_calls";
pub const ATTR_THEO_SUCCESS: &str = "theo.run.success";

// — Tool-call span attributes.
pub const ATTR_THEO_TOOL_NAME: &str = "theo.tool.name";
pub const ATTR_THEO_TOOL_CALL_ID: &str = "theo.tool.call_id";
pub const ATTR_THEO_TOOL_DURATION_MS: &str = "theo.tool.duration_ms";
pub const ATTR_THEO_TOOL_STATUS: &str = "theo.tool.status";
pub const ATTR_THEO_TOOL_REPLAYED: &str = "theo.tool.replayed";

// ---------------------------------------------------------------------------
// AgentRunSpan — semantic-convention-aligned span attributes builder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct AgentRunSpan {
    pub attributes: BTreeMap<String, Value>,
}

impl AgentRunSpan {
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize from an `AgentSpec` — populates `gen_ai.agent.id`,
    /// `gen_ai.agent.name`, `theo.agent.source`, `theo.agent.builtin`.
    pub fn from_spec(spec: &AgentSpec, run_id: &str) -> Self {
        let mut s = Self::new();
        s.set(ATTR_AGENT_ID, run_id);
        s.set(ATTR_AGENT_NAME, spec.name.clone());
        s.set(ATTR_THEO_AGENT_SOURCE, spec.source.as_str());
        s.set(
            ATTR_THEO_AGENT_BUILTIN,
            matches!(spec.source, theo_domain::agent_spec::AgentSpecSource::Builtin),
        );
        if let Some(model) = &spec.model_override {
            s.set(ATTR_REQUEST_MODEL, model.clone());
        }
        s
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) -> &mut Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Convert to `serde_json::Value` for direct payload embedding.
    pub fn to_json(&self) -> Value {
        Value::Object(
            self.attributes
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )
    }

    /// Format as a structured log line:
    /// `domain.action_state attr1=v1 attr2=v2 ...`
    /// Following Archon `{domain}.{action}_{state}` convention.
    pub fn to_log_line(&self, event_name: &str) -> String {
        let pairs: Vec<String> = self
            .attributes
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        format!("{} {}", event_name, pairs.join(" "))
    }
}

/// Helper: build attributes for an `llm.call` span.
pub fn llm_call_span(provider: &str, model: &str) -> AgentRunSpan {
    let mut s = AgentRunSpan::new();
    s.set(ATTR_SYSTEM, provider);
    s.set(ATTR_REQUEST_MODEL, model);
    s.set(ATTR_OPERATION_NAME, "chat");
    s
}

/// Helper: build attributes for a
/// `tool.call` span. Set on `ToolCallDispatched` and re-applied with
/// duration/status on `ToolCallCompleted`.
pub fn tool_call_span(tool_name: &str) -> AgentRunSpan {
    let mut s = AgentRunSpan::new();
    s.set(ATTR_OPERATION_NAME, "tool.call");
    s.set(ATTR_THEO_TOOL_NAME, tool_name);
    s
}

/// Helper: format a structured log event name following Archon convention.
/// `domain.action_state` — e.g. `subagent.spawn_started`.
pub fn log_event(domain: &str, action: &str, state: &str) -> String {
    format!("{}.{}_{}", domain, action, state)
}

// ---------------------------------------------------------------------------
// MetricsByAgent — per-agent breakdown for the dashboard (A4 gap)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub runs: u64,
    pub success: u64,
    pub failure: u64,
    pub total_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_llm_calls: u64,
    pub total_iterations: u64,
    pub total_duration_ms: u64,
}

impl AgentMetrics {
    /// Record a run completion.
    pub fn record(&mut self, success: bool, payload: &SubagentRunMetrics) {
        self.runs += 1;
        if success {
            self.success += 1;
        } else {
            self.failure += 1;
        }
        self.total_tokens += payload.tokens_used;
        self.total_input_tokens += payload.input_tokens;
        self.total_output_tokens += payload.output_tokens;
        self.total_llm_calls += payload.llm_calls;
        self.total_iterations += payload.iterations_used as u64;
        self.total_duration_ms += payload.duration_ms;
    }

    pub fn avg_tokens_per_run(&self) -> f64 {
        if self.runs == 0 {
            0.0
        } else {
            self.total_tokens as f64 / self.runs as f64
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.runs == 0 {
            0.0
        } else {
            self.success as f64 / self.runs as f64
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SubagentRunMetrics {
    pub tokens_used: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub llm_calls: u64,
    pub iterations_used: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsByAgent {
    pub by_agent: BTreeMap<String, AgentMetrics>,
}

impl MetricsByAgent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, agent_name: &str, success: bool, payload: SubagentRunMetrics) {
        self.by_agent
            .entry(agent_name.to_string())
            .or_default()
            .record(success, &payload);
    }

    pub fn get(&self, agent_name: &str) -> Option<&AgentMetrics> {
        self.by_agent.get(agent_name)
    }

    /// Top-N agents by total tokens consumed (cost dashboard).
    pub fn top_by_tokens(&self, n: usize) -> Vec<(&String, &AgentMetrics)> {
        let mut items: Vec<_> = self.by_agent.iter().collect();
        items.sort_by_key(|x| std::cmp::Reverse(x.1.total_tokens));
        items.into_iter().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::agent_spec::{AgentSpec, AgentSpecSource};

    #[test]
    fn attribute_constants_match_otel_genai_spec() {
        assert_eq!(ATTR_SYSTEM, "gen_ai.system");
        assert_eq!(ATTR_AGENT_ID, "gen_ai.agent.id");
        assert_eq!(ATTR_AGENT_NAME, "gen_ai.agent.name");
        assert_eq!(ATTR_USAGE_INPUT_TOKENS, "gen_ai.usage.input_tokens");
        assert_eq!(ATTR_USAGE_OUTPUT_TOKENS, "gen_ai.usage.output_tokens");
        assert_eq!(ATTR_REQUEST_MODEL, "gen_ai.request.model");
    }

    #[test]
    fn agent_run_span_from_spec_populates_required_attrs() {
        let spec = AgentSpec {
            name: "explorer".into(),
            description: "test".into(),
            system_prompt: "sp".into(),
            capability_set: theo_domain::capability::CapabilitySet::read_only(),
            model_override: Some("claude-sonnet-4-7".into()),
            max_iterations: 30,
            timeout_secs: 300,
            source: AgentSpecSource::Builtin,
            output_format: None,
            output_format_strict: None,
            mcp_servers: Vec::new(),
            isolation: None,
            isolation_base_branch: None,
            hooks: None,
        };
        let span = AgentRunSpan::from_spec(&spec, "run-abc");
        assert_eq!(span.attributes[ATTR_AGENT_ID], "run-abc");
        assert_eq!(span.attributes[ATTR_AGENT_NAME], "explorer");
        assert_eq!(span.attributes[ATTR_THEO_AGENT_SOURCE], "builtin");
        assert_eq!(span.attributes[ATTR_THEO_AGENT_BUILTIN], true);
        assert_eq!(span.attributes[ATTR_REQUEST_MODEL], "claude-sonnet-4-7");
    }

    #[test]
    fn agent_run_span_no_model_override_skips_request_model() {
        let spec = AgentSpec::on_demand("temp", "x");
        let span = AgentRunSpan::from_spec(&spec, "run-1");
        assert!(!span.attributes.contains_key(ATTR_REQUEST_MODEL));
    }

    #[test]
    fn agent_run_span_to_json_serializes_attributes() {
        let mut span = AgentRunSpan::new();
        span.set(ATTR_AGENT_NAME, "x");
        span.set(ATTR_USAGE_INPUT_TOKENS, 100u64);
        let json = span.to_json();
        assert_eq!(json[ATTR_AGENT_NAME], "x");
        assert_eq!(json[ATTR_USAGE_INPUT_TOKENS], 100);
    }

    #[test]
    fn llm_call_span_has_gen_ai_attrs() {
        let span = llm_call_span("anthropic", "claude-sonnet-4-7");
        assert_eq!(span.attributes[ATTR_SYSTEM], "anthropic");
        assert_eq!(span.attributes[ATTR_REQUEST_MODEL], "claude-sonnet-4-7");
        assert_eq!(span.attributes[ATTR_OPERATION_NAME], "chat");
    }

    #[test]
    fn log_event_follows_domain_action_state_convention() {
        assert_eq!(log_event("subagent", "spawn", "started"), "subagent.spawn_started");
        assert_eq!(
            log_event("workflow", "step", "completed"),
            "workflow.step_completed"
        );
    }

    #[test]
    fn metrics_by_agent_records_per_agent() {
        let mut m = MetricsByAgent::new();
        m.record(
            "explorer",
            true,
            SubagentRunMetrics {
                tokens_used: 1000,
                llm_calls: 3,
                iterations_used: 5,
                ..Default::default()
            },
        );
        m.record(
            "explorer",
            true,
            SubagentRunMetrics {
                tokens_used: 2000,
                llm_calls: 2,
                iterations_used: 3,
                ..Default::default()
            },
        );
        m.record(
            "implementer",
            false,
            SubagentRunMetrics {
                tokens_used: 5000,
                ..Default::default()
            },
        );
        let exp = m.get("explorer").unwrap();
        assert_eq!(exp.runs, 2);
        assert_eq!(exp.success, 2);
        assert_eq!(exp.total_tokens, 3000);
        assert_eq!(exp.avg_tokens_per_run(), 1500.0);
        assert_eq!(exp.success_rate(), 1.0);

        let imp = m.get("implementer").unwrap();
        assert_eq!(imp.failure, 1);
        assert_eq!(imp.success_rate(), 0.0);
    }

    #[test]
    fn metrics_by_agent_top_by_tokens_returns_sorted() {
        let mut m = MetricsByAgent::new();
        m.record("a", true, SubagentRunMetrics { tokens_used: 100, ..Default::default() });
        m.record("b", true, SubagentRunMetrics { tokens_used: 500, ..Default::default() });
        m.record("c", true, SubagentRunMetrics { tokens_used: 250, ..Default::default() });
        let top = m.top_by_tokens(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "b"); // 500
        assert_eq!(top[1].0, "c"); // 250
    }

    #[test]
    fn metrics_by_agent_get_returns_none_for_unknown() {
        let m = MetricsByAgent::new();
        assert!(m.get("nonexistent").is_none());
    }

    #[test]
    fn agent_metrics_avg_tokens_zero_when_no_runs() {
        let m = AgentMetrics::default();
        assert_eq!(m.avg_tokens_per_run(), 0.0);
        assert_eq!(m.success_rate(), 0.0);
    }

    #[test]
    fn span_to_log_line_includes_event_name_and_attrs() {
        let mut span = AgentRunSpan::new();
        span.set(ATTR_AGENT_NAME, "explorer");
        let line = span.to_log_line("subagent.spawn_started");
        assert!(line.starts_with("subagent.spawn_started"));
        assert!(line.contains("gen_ai.agent.name"));
    }
}
