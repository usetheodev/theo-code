import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  DerivedMetrics,
  RunSummary,
  TrajectoryProjection,
} from "../types";

interface ObservabilityState {
  runs: RunSummary[];
  selectedRun: TrajectoryProjection | null;
  loading: boolean;
  error: string | null;
}

const initialState: ObservabilityState = {
  runs: [],
  selectedRun: null,
  loading: false,
  error: null,
};

/**
 * Encapsulates all Tauri invokes for the observability dashboard.
 * Components never call `invoke` directly — they use this hook so that
 * changes to the backend contract live in a single place.
 */
export function useObservability() {
  const [state, setState] = useState<ObservabilityState>(initialState);

  const loadRuns = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const runs = await invoke<RunSummary[]>("list_runs");
      setState((s) => ({ ...s, runs, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const selectRun = useCallback(async (runId: string) => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const traj = await invoke<TrajectoryProjection>("get_run_trajectory", {
        runId,
      });
      setState((s) => ({ ...s, selectedRun: traj, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const compareRuns = useCallback(async (runIds: string[]) => {
    try {
      return await invoke<DerivedMetrics[]>("compare_runs", { runIds });
    } catch (e) {
      setState((s) => ({ ...s, error: String(e) }));
      return [];
    }
  }, []);

  return {
    runs: state.runs,
    selectedRun: state.selectedRun,
    loading: state.loading,
    error: state.error,
    loadRuns,
    selectRun,
    compareRuns,
  };
}
