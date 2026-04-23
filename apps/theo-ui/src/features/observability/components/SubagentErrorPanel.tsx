import type { ErrorTaxonomy, SubagentMetrics } from "../types";

interface Props {
  subagent: SubagentMetrics;
  errors: ErrorTaxonomy;
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
  if (n >= 1000) return `${(n / 1000).toFixed(1)}s`;
  return `${n.toFixed(0)}ms`;
}

export function SubagentErrorPanel({ subagent, errors }: Props) {
  const categories: Array<[string, number, string]> = [
    ["network", errors.network_errors, "bg-accent-blue/60"],
    ["llm", errors.llm_errors, "bg-accent-purple/60"],
    ["tool", errors.tool_errors, "bg-accent-yellow/60"],
    ["sandbox", errors.sandbox_errors, "bg-accent-red/60"],
    ["budget", errors.budget_errors, "bg-accent-red/60"],
    ["validation", errors.validation_errors, "bg-accent-yellow/60"],
    ["failure_mode", errors.failure_mode_errors, "bg-accent-red/60"],
    ["other", errors.other_errors, "bg-text-3/40"],
  ].filter(([_, v]) => (v as number) > 0) as Array<[string, number, string]>;
  const errorTotal = errors.total_errors;

  return (
    <div className="grid grid-cols-2 gap-3">
      <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="subagent-panel">
        <h3 className="text-[13px] font-semibold text-text-1 mb-3">Sub-agent Efficiency</h3>
        {subagent.spawned === 0 ? (
          <div className="text-text-3 text-[12px] py-4">No sub-agents spawned in this run.</div>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            <Stat label="Spawned" value={subagent.spawned.toString()} />
            <Stat
              label="Success rate"
              value={`${(subagent.success_rate * 100).toFixed(0)}%`}
              hint={`${subagent.succeeded}/${subagent.spawned}`}
              color={subagent.success_rate > 0.9 ? "text-accent-green" : subagent.success_rate > 0.5 ? "text-accent-yellow" : "text-accent-red"}
            />
            <Stat label="Avg duration" value={fmt(subagent.avg_duration_ms)} />
            <Stat label="Max duration" value={fmt(subagent.max_duration_ms)} />
          </div>
        )}
      </div>
      <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="error-taxonomy">
        <h3 className="text-[13px] font-semibold text-text-1 mb-3">
          Error Taxonomy
          <span className="ml-2 text-[11px] text-text-3">
            ({errorTotal} total)
          </span>
        </h3>
        {errorTotal === 0 ? (
          <div className="text-accent-green text-[12px] py-4">No errors in this run ✓</div>
        ) : (
          <div className="flex flex-col gap-1.5">
            {categories.map(([name, count, color]) => {
              const pct = (count / errorTotal) * 100;
              return (
                <div key={name} className="flex items-center gap-3">
                  <span className="text-[12px] text-text-2 w-24 capitalize">{name}</span>
                  <div className="flex-1 h-3 bg-surface-3 rounded overflow-hidden">
                    <div className={`h-3 ${color}`} style={{ width: `${pct}%` }} />
                  </div>
                  <span className="font-mono text-[11px] text-text-1 w-10 text-right">{count}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
