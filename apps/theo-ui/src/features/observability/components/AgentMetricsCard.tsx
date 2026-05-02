// Phase 15 (sota-gaps): per-agent metrics card.
//
// Single-card view of an agent's aggregated stats. Used by AgentsPage to
// render a grid of registered sub-agents and their cost/success breakdown.

import type { AgentStats } from "../hooks/useAgents";

interface AgentMetricsCardProps {
  stats: AgentStats;
  onSelect?: (agentName: string) => void;
  selected?: boolean;
}

function formatNumber(v: number): string {
  if (Math.abs(v) >= 1000) return Math.round(v).toString();
  return v.toFixed(1);
}

function formatPercent(v: number): string {
  return `${(v * 100).toFixed(0)}%`;
}

function colorForSuccessRate(rate: number, runCount: number): string {
  // No data → neutral gray
  if (runCount === 0) return "text-text-3";
  if (rate >= 0.9) return "text-accent-green";
  if (rate >= 0.6) return "text-accent-yellow";
  return "text-accent-red";
}

function sourceLabel(source: string): string {
  switch (source) {
    case "builtin":
      return "Built-in";
    case "project":
      return "Project";
    case "global":
      return "Global";
    case "on_demand":
      return "On-demand";
    default:
      return source;
  }
}

export function AgentMetricsCard({
  stats,
  onSelect,
  selected = false,
}: AgentMetricsCardProps) {
  const successColor = colorForSuccessRate(stats.success_rate, stats.run_count);
  const interactive = !!onSelect;
  const baseClass = `rounded-xl border p-4 flex flex-col gap-2 min-w-[220px] transition-colors ${
    selected
      ? "border-accent-blue bg-surface-3"
      : "border-border bg-surface-2"
  } ${interactive ? "cursor-pointer hover:border-accent-blue" : ""}`;

  return (
    <div
      data-testid={`agent-card-${stats.agent_name}`}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={interactive ? () => onSelect?.(stats.agent_name) : undefined}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect?.(stats.agent_name);
              }
            }
          : undefined
      }
      className={baseClass}
    >
      <header className="flex items-center justify-between">
        <span className="text-sm font-semibold text-text-1">
          {stats.agent_name}
        </span>
        <span className="text-[10px] uppercase tracking-wide text-text-3">
          {sourceLabel(stats.agent_source)}
        </span>
      </header>

      <div className="grid grid-cols-2 gap-2 text-xs text-text-2">
        <div>
          <div className="text-[10px] uppercase text-text-3">Runs</div>
          <div className="text-lg font-semibold text-text-1">
            {stats.run_count}
          </div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-text-3">Success</div>
          <div className={`text-lg font-semibold ${successColor}`}>
            {formatPercent(stats.success_rate)}
          </div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-text-3">Avg tokens</div>
          <div className="text-base text-text-1">
            {formatNumber(stats.avg_tokens_per_run)}
          </div>
        </div>
        <div>
          <div className="text-[10px] uppercase text-text-3">Avg iter</div>
          <div className="text-base text-text-1">
            {formatNumber(stats.avg_iterations_per_run)}
          </div>
        </div>
      </div>

      <footer className="flex flex-wrap gap-1 text-[10px] mt-1">
        {stats.running_count > 0 && (
          <span className="rounded bg-accent-blue/10 text-accent-blue px-1.5 py-0.5">
            {stats.running_count} running
          </span>
        )}
        {stats.failure_count > 0 && (
          <span className="rounded bg-accent-red/10 text-accent-red px-1.5 py-0.5">
            {stats.failure_count} failed
          </span>
        )}
        {stats.cancelled_count > 0 && (
          <span className="rounded bg-accent-yellow/10 text-accent-yellow px-1.5 py-0.5">
            {stats.cancelled_count} cancelled
          </span>
        )}
        {stats.abandoned_count > 0 && (
          <span className="rounded bg-text-3/10 text-text-3 px-1.5 py-0.5">
            {stats.abandoned_count} abandoned
          </span>
        )}
      </footer>
    </div>
  );
}
