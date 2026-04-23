// Observability TypeScript types that mirror the Rust structs in
// `theo-agent-runtime::observability` (see
// `crates/theo-agent-runtime/src/observability/`).
//
// Keep in sync with `theo_application::use_cases::observability_ui`.

export type EventKind =
  | "Lifecycle"
  | "Tooling"
  | "Reasoning"
  | "Context"
  | "Failure"
  | "Streaming";

export type StepOutcome =
  | "Success"
  | { Failure: { retryable: boolean } }
  | "Timeout"
  | "Skipped";

export interface SurrogateMetric {
  value: number;
  confidence: number;
  numerator: number;
  denominator: number;
  is_surrogate: boolean;
  caveat: string;
}

export interface DerivedMetrics {
  doom_loop_frequency: SurrogateMetric;
  llm_efficiency: SurrogateMetric;
  context_waste_ratio: SurrogateMetric;
  hypothesis_churn_rate: SurrogateMetric;
  time_to_first_tool_ms: SurrogateMetric;
}

export interface ProjectedStep {
  sequence: number;
  event_type: string;
  event_kind: EventKind | null;
  timestamp: number;
  entity_id: string;
  payload_summary: string;
  duration_ms: number | null;
  tool_name: string | null;
  outcome: StepOutcome | null;
}

export interface MissingRange {
  start: number;
  end: number;
}

export interface IntegrityReport {
  complete: boolean;
  total_events_expected: number;
  total_events_received: number;
  missing_sequences: MissingRange[];
  drop_sentinels_found: number;
  writer_recoveries_found: number;
  confidence: number;
  schema_version: number;
}

export interface TrajectoryProjection {
  run_id: string;
  trajectory_id: string;
  steps: ProjectedStep[];
  integrity: IntegrityReport;
}

export interface RunSummary {
  run_id: string;
  timestamp: number;
  success: boolean;
  total_steps: number;
  total_tool_calls: number;
  duration_ms: number;
  metrics: DerivedMetrics;
}

// --- Full RunReport: everything the backend computes for a single run ---

export interface TokenMetrics {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  reasoning_tokens: number;
  total_cost_usd: number;
  cache_hit_rate: number;
  tokens_per_successful_edit: number;
}

export interface PhaseMetric {
  iterations: number;
  duration_ms: number;
  pct: number;
}

export interface BudgetUtilization {
  iterations_pct: number;
  tokens_pct: number;
  time_pct: number;
}

export interface LoopMetrics {
  phase_distribution: Record<string, PhaseMetric>;
  total_iterations: number;
  done_blocked_count: number;
  convergence_rate: number;
  budget_utilization: BudgetUtilization;
  evolution_attempts: number;
  evolution_success: boolean;
}

export interface ToolBreakdown {
  tool_name: string;
  call_count: number;
  success_count: number;
  failure_count: number;
  avg_latency_ms: number;
  max_latency_ms: number;
  retry_count: number;
  success_rate: number;
}

export interface ContextHealthMetrics {
  avg_context_size_tokens: number;
  max_context_size_tokens: number;
  context_growth_rate: number;
  compaction_count: number;
  compaction_savings_ratio: number;
  refetch_rate: number;
  action_repetition_rate: number;
  usefulness_avg: number;
}

export interface MemoryMetrics {
  episodes_injected: number;
  episodes_created: number;
  hypotheses_formed: number;
  hypotheses_invalidated: number;
  hypotheses_active: number;
  constraints_learned: number;
  failure_fingerprints_new: number;
  failure_fingerprints_recurrent: number;
}

export interface SubagentMetrics {
  spawned: number;
  succeeded: number;
  failed: number;
  avg_duration_ms: number;
  max_duration_ms: number;
  success_rate: number;
}

export interface ErrorTaxonomy {
  total_errors: number;
  network_errors: number;
  llm_errors: number;
  tool_errors: number;
  sandbox_errors: number;
  budget_errors: number;
  validation_errors: number;
  failure_mode_errors: number;
  other_errors: number;
}

export interface RunReport {
  surrogate_metrics: DerivedMetrics;
  token_metrics: TokenMetrics;
  loop_metrics: LoopMetrics;
  tool_breakdown: ToolBreakdown[];
  context_health: ContextHealthMetrics;
  memory_metrics: MemoryMetrics;
  subagent_metrics: SubagentMetrics;
  error_taxonomy: ErrorTaxonomy;
  integrity: IntegrityReport;
}

/** Default empty SurrogateMetric used when data is missing. */
export const emptyMetric: SurrogateMetric = {
  value: 0,
  confidence: 0,
  numerator: 0,
  denominator: 0,
  is_surrogate: true,
  caveat: "",
};

export const emptyDerivedMetrics: DerivedMetrics = {
  doom_loop_frequency: emptyMetric,
  llm_efficiency: emptyMetric,
  context_waste_ratio: emptyMetric,
  hypothesis_churn_rate: emptyMetric,
  time_to_first_tool_ms: emptyMetric,
};
