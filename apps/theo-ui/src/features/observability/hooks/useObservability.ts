import { useCallback, useState } from "react";

import type {
  DerivedMetrics,
  RunReport,
  RunSummary,
  TrajectoryProjection,
} from "../types";

export interface SystemStats {
  total_runs: number;
  successful_runs: number;
  failed_runs: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cache_read_tokens: number;
  total_tool_calls: number;
  total_tool_failures: number;
  total_duration_ms: number;
  total_subagent_spawned: number;
  total_subagent_succeeded: number;
  total_errors: number;
  errors_by_category: Record<string, number>;
  tools_by_usage: Array<[string, number]>;
  tools_by_failure_rate: Array<[string, number]>;
  avg_llm_efficiency: number;
  avg_cache_hit_rate: number;
  avg_iterations_per_run: number;
  total_episodes_injected: number;
  total_hypotheses_formed: number;
  total_hypotheses_invalidated: number;
  total_constraints_learned: number;
  total_fingerprints_new: number;
  total_fingerprints_recurrent: number;
}

interface ObservabilityState {
  runs: RunSummary[];
  selectedRun: TrajectoryProjection | null;
  selectedReport: RunReport | null;
  systemStats: SystemStats | null;
  loading: boolean;
  error: string | null;
}

const initialState: ObservabilityState = {
  runs: [],
  selectedRun: null,
  selectedReport: null,
  systemStats: null,
  loading: false,
  error: null,
};

function isTauri(): boolean {
  return typeof window !== "undefined"
    && (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ !== undefined;
}

async function callBackend<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }
  switch (command) {
    case "list_runs": {
      const r = await fetch("/api/list_runs");
      if (!r.ok) throw new Error(`list_runs failed: ${r.status}`);
      return (await r.json()) as T;
    }
    case "get_run_trajectory": {
      const id = args?.runId as string;
      const r = await fetch(`/api/run/${encodeURIComponent(id)}/trajectory`);
      if (!r.ok) throw new Error(`get_run_trajectory failed: ${r.status}`);
      return (await r.json()) as T;
    }
    case "get_run_metrics": {
      const id = args?.runId as string;
      const r = await fetch(`/api/run/${encodeURIComponent(id)}/metrics`);
      if (!r.ok) throw new Error(`get_run_metrics failed: ${r.status}`);
      return (await r.json()) as T;
    }
    case "get_run_report": {
      const id = args?.runId as string;
      const r = await fetch(`/api/run/${encodeURIComponent(id)}/report`);
      if (!r.ok) throw new Error(`get_run_report failed: ${r.status}`);
      return (await r.json()) as T;
    }
    case "get_system_stats": {
      const r = await fetch("/api/system/stats");
      if (!r.ok) throw new Error(`get_system_stats failed: ${r.status}`);
      return (await r.json()) as T;
    }
    case "compare_runs": {
      const r = await fetch("/api/runs/compare", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ run_ids: args?.runIds ?? [] }),
      });
      if (!r.ok) throw new Error(`compare_runs failed: ${r.status}`);
      return (await r.json()) as T;
    }
    default:
      throw new Error(`Unknown command: ${command}`);
  }
}

export function useObservability() {
  const [state, setState] = useState<ObservabilityState>(initialState);

  const loadRuns = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const runs = await callBackend<RunSummary[]>("list_runs");
      setState((s) => ({ ...s, runs, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const loadSystemStats = useCallback(async () => {
    try {
      const systemStats = await callBackend<SystemStats>("get_system_stats");
      setState((s) => ({ ...s, systemStats }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e) }));
    }
  }, []);

  const selectRun = useCallback(async (runId: string) => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const [traj, report] = await Promise.all([
        callBackend<TrajectoryProjection>("get_run_trajectory", { runId }),
        callBackend<RunReport>("get_run_report", { runId }).catch(() => null),
      ]);
      setState((s) => ({ ...s, selectedRun: traj, selectedReport: report, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const compareRuns = useCallback(async (runIds: string[]) => {
    try {
      return await callBackend<DerivedMetrics[]>("compare_runs", { runIds });
    } catch (e) {
      setState((s) => ({ ...s, error: String(e) }));
      return [];
    }
  }, []);

  return {
    runs: state.runs,
    selectedRun: state.selectedRun,
    selectedReport: state.selectedReport,
    systemStats: state.systemStats,
    loading: state.loading,
    error: state.error,
    loadRuns,
    loadSystemStats,
    selectRun,
    compareRuns,
  };
}
