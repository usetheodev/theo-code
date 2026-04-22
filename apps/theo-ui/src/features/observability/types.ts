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
