// Phase 15 (sota-gaps): per-agent dashboard hook.
//
// Wraps the `/api/agents` and `/api/agents/:name` endpoints exposed by the
// CLI dashboard server (apps/theo-cli/src/dashboard.rs). Tauri shell support
// is intentionally omitted — this dashboard is browser-only (CLI mode).

import { useCallback, useEffect, useRef, useState } from "react";

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

export interface SubagentRunEvent {
  type: "subagent_run_added" | "subagent_run_updated";
  agent_name: string;
  run_id: string;
  status: string;
  tokens_used: number;
}

interface AgentsState {
  agents: AgentStats[];
  selected: AgentDetail | null;
  selectedRuns: RecentRun[];
  liveEvents: SubagentRunEvent[];
  loading: boolean;
  error: string | null;
}

const initial: AgentsState = {
  agents: [],
  selected: null,
  selectedRuns: [],
  liveEvents: [],
  loading: false,
  error: null,
};

const MAX_LIVE_EVENTS = 50;

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
      const [detailRes, runsRes] = await Promise.all([
        fetch(`/api/agents/${encodeURIComponent(agentName)}`),
        fetch(`/api/agents/${encodeURIComponent(agentName)}/runs`),
      ]);
      if (!detailRes.ok) throw new Error(`get agent failed: ${detailRes.status}`);
      const detail = (await detailRes.json()) as AgentDetail;
      const runs = runsRes.ok ? ((await runsRes.json()) as RecentRun[]) : [];
      setState((s) => ({
        ...s,
        selected: detail,
        selectedRuns: runs,
        loading: false,
      }));
    } catch (e) {
      setState((s) => ({ ...s, error: String(e), loading: false }));
    }
  }, []);

  const clearSelection = useCallback(() => {
    setState((s) => ({ ...s, selected: null, selectedRuns: [] }));
  }, []);

  // Live event stream — auto-attached when the hook mounts. Browser-only.
  const eventSourceRef = useRef<EventSource | null>(null);
  useEffect(() => {
    if (typeof EventSource === "undefined") return;
    const es = new EventSource("/api/agents/events");
    eventSourceRef.current = es;
    const handleAdd = (ev: MessageEvent) => {
      try {
        const payload = JSON.parse(ev.data) as SubagentRunEvent;
        setState((s) => ({
          ...s,
          liveEvents: [payload, ...s.liveEvents].slice(0, MAX_LIVE_EVENTS),
        }));
      } catch {
        /* ignore malformed event */
      }
    };
    es.addEventListener("subagent_run_added", handleAdd);
    es.addEventListener("subagent_run_updated", handleAdd);
    return () => {
      es.close();
      eventSourceRef.current = null;
    };
  }, []);

  return {
    agents: state.agents,
    selected: state.selected,
    selectedRuns: state.selectedRuns,
    liveEvents: state.liveEvents,
    loading: state.loading,
    error: state.error,
    loadAgents,
    selectAgent,
    clearSelection,
  };
}
