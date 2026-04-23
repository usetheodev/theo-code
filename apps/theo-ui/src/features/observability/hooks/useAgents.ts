// Phase 15 (sota-gaps): per-agent dashboard hook.
//
// Wraps the `/api/agents` and `/api/agents/:name` endpoints exposed by the
// CLI dashboard server (apps/theo-cli/src/dashboard.rs). Tauri shell support
// is intentionally omitted — this dashboard is browser-only (CLI mode).

import { useCallback, useState } from "react";

export interface AgentStats {
  agent_name: string;
  agent_source: string;
  run_count: number;
  success_count: number;
  failure_count: number;
  cancelled_count: number;
  abandoned_count: number;
  running_count: number;
  total_tokens_used: number;
  total_iterations_used: number;
  avg_tokens_per_run: number;
  avg_iterations_per_run: number;
  success_rate: number;
  last_started_at: number;
}

export interface RecentRun {
  run_id: string;
  status: string;
  started_at: number;
  finished_at: number | null;
  iterations_used: number;
  tokens_used: number;
  objective: string;
  summary: string | null;
}

export interface AgentDetail {
  stats: AgentStats;
  recent_runs: RecentRun[];
}

interface AgentsState {
  agents: AgentStats[];
  selected: AgentDetail | null;
  loading: boolean;
  error: string | null;
}

const initial: AgentsState = {
  agents: [],
  selected: null,
  loading: false,
  error: null,
};

export function useAgents() {
  const [state, setState] = useState<AgentsState>(initial);

  const loadAgents = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const r = await fetch("/api/agents");
      if (!r.ok) throw new Error(`list agents failed: ${r.status}`);
      const agents = (await r.json()) as AgentStats[];
      setState((s) => ({ ...s, agents, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const selectAgent = useCallback(async (agentName: string) => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const r = await fetch(`/api/agents/${encodeURIComponent(agentName)}`);
      if (!r.ok) throw new Error(`get agent failed: ${r.status}`);
      const detail = (await r.json()) as AgentDetail;
      setState((s) => ({ ...s, selected: detail, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const clearSelection = useCallback(() => {
    setState((s) => ({ ...s, selected: null }));
  }, []);

  return {
    agents: state.agents,
    selected: state.selected,
    loading: state.loading,
    error: state.error,
    loadAgents,
    selectAgent,
    clearSelection,
  };
}
