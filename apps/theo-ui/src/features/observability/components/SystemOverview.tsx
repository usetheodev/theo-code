import type { SystemStats } from "../hooks/useObservability";

interface Props {
  stats: SystemStats;
}

function Stat({ label, value, hint, color }: { label: string; value: string; hint?: string; color?: string }) {
  return (
    <div className="bg-surface-3 rounded p-3">
      <div className="text-[10px] text-text-3 uppercase">{label}</div>
      <div className={`text-[18px] font-semibold ${color ?? "text-text-1"}`}>{value}</div>
      {hint && <div className="text-[10px] text-text-3 mt-1">{hint}</div>}
    </div>
  );
}

function fmt(n: number): string {
  if (n === 0) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return n.toString();
}

function pct(n: number): string {
  return `${(n * 100).toFixed(1)}%`;
}

export function SystemOverview({ stats }: Props) {
  const successRate = stats.total_runs > 0 ? stats.successful_runs / stats.total_runs : 0;
  const toolFailureRate = stats.total_tool_calls > 0 ? stats.total_tool_failures / stats.total_tool_calls : 0;
  const subagentSuccessRate = stats.total_subagent_spawned > 0
    ? stats.total_subagent_succeeded / stats.total_subagent_spawned
    : 0;
  const memoryROI = stats.total_episodes_injected + stats.total_constraints_learned + stats.total_hypotheses_formed;

  return (
    <div className="bg-surface-2 rounded-xl border border-accent-blue/40 p-4" data-testid="system-overview">
      <h3 className="text-[13px] font-semibold text-accent-blue mb-3">System Overview — aggregate of {stats.total_runs} runs</h3>

      <div className="grid grid-cols-5 gap-3 mb-4">
        <Stat
          label="Runs total"
          value={stats.total_runs.toString()}
          hint={`${stats.successful_runs} ✓ / ${stats.failed_runs} ✗`}
        />
        <Stat
          label="Run success rate"
          value={pct(successRate)}
          color={successRate > 0.7 ? "text-accent-green" : successRate > 0.4 ? "text-accent-yellow" : "text-accent-red"}
        />
        <Stat
          label="Total tokens"
          value={fmt(stats.total_input_tokens + stats.total_output_tokens + stats.total_cache_read_tokens)}
          hint={`${fmt(stats.total_input_tokens)} in · ${fmt(stats.total_output_tokens)} out`}
        />
        <Stat
          label="Avg cache hit"
          value={pct(stats.avg_cache_hit_rate)}
          color={stats.avg_cache_hit_rate > 0.3 ? "text-accent-green" : stats.avg_cache_hit_rate > 0.1 ? "text-accent-yellow" : "text-accent-red"}
        />
        <Stat
          label="Avg iterations"
          value={stats.avg_iterations_per_run.toFixed(1)}
          hint="per run"
        />
      </div>

      <div className="grid grid-cols-5 gap-3 mb-4">
        <Stat
          label="Total tool calls"
          value={fmt(stats.total_tool_calls)}
          hint={`${stats.total_tool_failures} failed`}
        />
        <Stat
          label="Tool failure rate"
          value={pct(toolFailureRate)}
          color={toolFailureRate < 0.05 ? "text-accent-green" : toolFailureRate < 0.15 ? "text-accent-yellow" : "text-accent-red"}
        />
        <Stat
          label="Avg LLM efficiency"
          value={stats.avg_llm_efficiency.toFixed(2)}
          color={stats.avg_llm_efficiency > 0.5 ? "text-accent-green" : stats.avg_llm_efficiency > 0.2 ? "text-accent-yellow" : "text-accent-red"}
        />
        <Stat
          label="Sub-agents"
          value={stats.total_subagent_spawned.toString()}
          hint={stats.total_subagent_spawned > 0 ? `${pct(subagentSuccessRate)} success` : "none used"}
          color={stats.total_subagent_spawned === 0 ? "text-text-3" : subagentSuccessRate > 0.8 ? "text-accent-green" : "text-accent-yellow"}
        />
        <Stat
          label="Errors total"
          value={stats.total_errors.toString()}
          color={stats.total_errors === 0 ? "text-accent-green" : stats.total_errors < 5 ? "text-accent-yellow" : "text-accent-red"}
        />
      </div>

      <div className="grid grid-cols-2 gap-4">
        <div>
          <div className="text-[11px] text-text-3 uppercase tracking-wide mb-2">Top tools (by usage)</div>
          <div className="flex flex-col gap-1">
            {stats.tools_by_usage.slice(0, 5).map(([name, n]) => (
              <div key={name} className="flex items-center gap-3">
                <span className="text-[12px] text-text-2 w-28 font-mono">{name}</span>
                <div className="flex-1 h-3 bg-surface-3 rounded overflow-hidden">
                  <div
                    className="h-3 bg-accent-blue/60"
                    style={{
                      width: `${(n / (stats.tools_by_usage[0]?.[1] ?? 1)) * 100}%`,
                    }}
                  />
                </div>
                <span className="font-mono text-[11px] text-text-1 w-10 text-right">{n}</span>
              </div>
            ))}
          </div>
        </div>
        <div>
          <div className="text-[11px] text-text-3 uppercase tracking-wide mb-2">Memory activity</div>
          <div className="grid grid-cols-2 gap-2 text-[11px]">
            <div className="flex justify-between">
              <span className="text-text-3">Episodes injected</span>
              <span className="font-mono">{stats.total_episodes_injected}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-3">Hypotheses formed</span>
              <span className="font-mono">{stats.total_hypotheses_formed}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-3">Hypotheses killed</span>
              <span className="font-mono">{stats.total_hypotheses_invalidated}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-3">Constraints learned</span>
              <span className="font-mono">{stats.total_constraints_learned}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-3">New fingerprints</span>
              <span className="font-mono">{stats.total_fingerprints_new}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-3">Recurrent</span>
              <span className="font-mono text-accent-red">{stats.total_fingerprints_recurrent}</span>
            </div>
            <div className="flex justify-between col-span-2 mt-1 pt-1 border-t border-border">
              <span className="text-text-3">Memory ROI signal</span>
              <span className={`font-mono ${memoryROI > 0 ? "text-accent-green" : "text-accent-yellow"}`}>
                {memoryROI > 0 ? "active" : "idle"}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
