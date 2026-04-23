// Phase 15 (sota-gaps): per-agent observability page.
//
// Lists every sub-agent that has at least one persisted run, plus a detail
// pane for the selected agent (recent runs + summary).

import { useEffect } from "react";

import { AgentMetricsCard } from "../components/AgentMetricsCard";
import { useAgents } from "../hooks/useAgents";

function formatTimestamp(unixSeconds: number): string {
  if (!unixSeconds) return "—";
  return new Date(unixSeconds * 1000).toLocaleString();
}

export function AgentsPage() {
  const {
    agents,
    selected,
    loading,
    error,
    loadAgents,
    selectAgent,
    clearSelection,
  } = useAgents();

  useEffect(() => {
    loadAgents();
  }, [loadAgents]);

  return (
    <div className="flex flex-col h-full">
      <header className="flex items-center justify-between border-b border-border px-6 py-4">
        <div>
          <h1 className="text-xl font-semibold text-text-1">Agents</h1>
          <p className="text-sm text-text-3">
            Per-agent cost, success rate, and recent runs.
          </p>
        </div>
        <button
          onClick={loadAgents}
          className="rounded border border-border bg-surface-2 px-3 py-1.5 text-sm hover:bg-surface-3"
          aria-label="Refresh agents list"
        >
          Refresh
        </button>
      </header>

      {error && (
        <div className="border-l-4 border-accent-red bg-accent-red/10 px-4 py-2 m-4 text-sm text-accent-red">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-auto p-6">
        {loading && agents.length === 0 && (
          <div className="text-text-3 text-sm">Loading agents…</div>
        )}

        {!loading && agents.length === 0 && (
          <div className="text-center text-text-3 py-16">
            <p>No persisted sub-agent runs yet.</p>
            <p className="text-xs mt-2">
              Runs appear here after the parent agent calls{" "}
              <code className="rounded bg-surface-3 px-1 py-0.5">delegate_task</code>.
            </p>
          </div>
        )}

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
          {agents.map((stats) => (
            <AgentMetricsCard
              key={stats.agent_name}
              stats={stats}
              onSelect={selectAgent}
              selected={selected?.stats.agent_name === stats.agent_name}
            />
          ))}
        </div>

        {selected && (
          <section
            data-testid="agent-detail"
            className="mt-8 rounded-xl border border-border bg-surface-2 p-4"
          >
            <header className="flex items-center justify-between mb-3">
              <h2 className="text-base font-semibold text-text-1">
                {selected.stats.agent_name} — recent runs
              </h2>
              <button
                onClick={clearSelection}
                className="text-xs text-text-3 hover:text-text-1"
                aria-label="Close agent detail"
              >
                Close
              </button>
            </header>

            <div className="grid grid-cols-3 gap-2 text-xs text-text-3 mb-3">
              <div>
                <div className="uppercase">Total tokens</div>
                <div className="text-text-1 text-sm">
                  {selected.stats.total_tokens_used.toLocaleString()}
                </div>
              </div>
              <div>
                <div className="uppercase">Last started</div>
                <div className="text-text-1 text-sm">
                  {formatTimestamp(selected.stats.last_started_at)}
                </div>
              </div>
              <div>
                <div className="uppercase">Source</div>
                <div className="text-text-1 text-sm">
                  {selected.stats.agent_source}
                </div>
              </div>
            </div>

            <table className="w-full text-xs">
              <thead className="text-text-3 uppercase tracking-wide">
                <tr>
                  <th className="text-left py-1.5">Run ID</th>
                  <th className="text-left py-1.5">Status</th>
                  <th className="text-right py-1.5">Iter</th>
                  <th className="text-right py-1.5">Tokens</th>
                  <th className="text-left py-1.5 pl-3">Objective</th>
                </tr>
              </thead>
              <tbody>
                {selected.recent_runs.map((run) => (
                  <tr
                    key={run.run_id}
                    className="border-t border-border/50"
                    data-testid={`recent-run-${run.run_id}`}
                  >
                    <td className="py-1.5 font-mono text-text-2">
                      {run.run_id}
                    </td>
                    <td className="py-1.5">
                      <span className="inline-block rounded bg-surface-3 px-1.5 py-0.5">
                        {run.status}
                      </span>
                    </td>
                    <td className="py-1.5 text-right">
                      {run.iterations_used}
                    </td>
                    <td className="py-1.5 text-right">{run.tokens_used}</td>
                    <td className="py-1.5 pl-3 text-text-2">
                      {run.objective.length > 80
                        ? run.objective.slice(0, 80) + "…"
                        : run.objective}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}
      </div>
    </div>
  );
}
