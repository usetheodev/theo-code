import type { LoopMetrics } from "../types";

interface Props {
  loop: LoopMetrics;
}

function Gauge({ label, value, warning = 0.7, critical = 0.9 }: { label: string; value: number; warning?: number; critical?: number }) {
  const pct = Math.min(100, Math.max(0, value * 100));
  const color = value >= critical
    ? "bg-accent-red"
    : value >= warning
      ? "bg-accent-yellow"
      : "bg-accent-green";
  return (
    <div className="bg-surface-3 rounded p-3">
      <div className="text-[10px] text-text-3 uppercase">{label}</div>
      <div className="flex items-baseline gap-2">
        <div className="text-[18px] font-semibold text-text-1">{pct.toFixed(1)}%</div>
      </div>
      <div className="h-1.5 bg-surface-4 rounded overflow-hidden mt-2">
        <div className={`h-1.5 ${color}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

export function LoopHealthPanel({ loop }: Props) {
  const phases = Object.entries(loop.phase_distribution);
  const phaseColor: Record<string, string> = {
    Explore: "bg-accent-blue/60",
    Edit: "bg-accent-green/60",
    Verify: "bg-accent-yellow/60",
    Done: "bg-accent-purple/60",
  };
  return (
    <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="loop-health">
      <h3 className="text-[13px] font-semibold text-text-1 mb-3">Loop Health</h3>
      <div className="grid grid-cols-2 gap-4">
        <div>
          <div className="text-[11px] text-text-3 uppercase tracking-wide mb-2">Phase distribution</div>
          <div className="flex flex-col gap-1.5">
            {phases.map(([name, m]) => (
              <div key={name} className="flex items-center gap-3">
                <span className="text-[12px] text-text-2 w-20">{name}</span>
                <div className="flex-1 h-3 bg-surface-3 rounded overflow-hidden flex items-center">
                  <div className={`h-3 ${phaseColor[name] ?? "bg-accent-blue/60"}`} style={{ width: `${m.pct * 100}%` }} />
                </div>
                <span className="font-mono text-[11px] text-text-1 w-12 text-right">{m.iterations}</span>
              </div>
            ))}
          </div>
          <div className="text-[11px] text-text-3 mt-3">
            {loop.total_iterations} iterations · convergence: <span className={loop.convergence_rate > 0 ? "text-accent-green" : "text-accent-red"}>{loop.convergence_rate > 0 ? "yes" : "no"}</span>
            {loop.done_blocked_count > 0 && <span className="text-accent-yellow"> · {loop.done_blocked_count} done blocks</span>}
            {loop.evolution_attempts > 0 && <span> · {loop.evolution_attempts} evolution attempts</span>}
          </div>
        </div>
        <div>
          <div className="text-[11px] text-text-3 uppercase tracking-wide mb-2">Budget utilization</div>
          <div className="grid grid-cols-3 gap-2">
            <Gauge label="Iterations" value={loop.budget_utilization.iterations_pct} />
            <Gauge label="Tokens" value={loop.budget_utilization.tokens_pct} />
            <Gauge label="Time" value={loop.budget_utilization.time_pct} />
          </div>
        </div>
      </div>
    </div>
  );
}
