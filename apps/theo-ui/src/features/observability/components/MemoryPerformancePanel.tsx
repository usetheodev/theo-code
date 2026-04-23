import type { MemoryMetrics } from "../types";

interface Props {
  memory: MemoryMetrics;
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

export function MemoryPerformancePanel({ memory }: Props) {
  const hypChurn = memory.hypotheses_formed > 0
    ? memory.hypotheses_invalidated / memory.hypotheses_formed
    : 0;
  const churnLabel = memory.hypotheses_formed === 0
    ? "—"
    : `${(hypChurn * 100).toFixed(0)}%`;
  const fpTotal = memory.failure_fingerprints_new + memory.failure_fingerprints_recurrent;
  const recurrentPct = fpTotal > 0 ? (memory.failure_fingerprints_recurrent / fpTotal) * 100 : 0;

  return (
    <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="memory-performance">
      <h3 className="text-[13px] font-semibold text-text-1 mb-3">Memory Performance</h3>
      <div className="grid grid-cols-4 gap-3">
        <Stat
          label="Episodes injected"
          value={memory.episodes_injected.toString()}
          hint="from past runs"
          color={memory.episodes_injected > 0 ? "text-accent-green" : "text-text-3"}
        />
        <Stat label="Episode created" value={memory.episodes_created.toString()} hint="this run" />
        <Stat
          label="Hypotheses formed"
          value={memory.hypotheses_formed.toString()}
          hint={memory.hypotheses_formed > 0 ? `${memory.hypotheses_active} active` : undefined}
        />
        <Stat
          label="Hyp. churn"
          value={churnLabel}
          hint="invalidated/formed"
          color={hypChurn > 0.7 ? "text-accent-red" : hypChurn > 0.3 ? "text-accent-yellow" : "text-accent-green"}
        />
        <Stat
          label="Constraints"
          value={memory.constraints_learned.toString()}
          hint="learned"
          color={memory.constraints_learned > 0 ? "text-accent-green" : "text-text-3"}
        />
        <Stat
          label="New fingerprints"
          value={memory.failure_fingerprints_new.toString()}
          hint="first-seen failures"
        />
        <Stat
          label="Recurrent"
          value={memory.failure_fingerprints_recurrent.toString()}
          hint={fpTotal > 0 ? `${recurrentPct.toFixed(0)}% of total` : undefined}
          color={memory.failure_fingerprints_recurrent > 0 ? "text-accent-red" : "text-text-3"}
        />
        <Stat
          label="Memory ROI"
          value={memory.episodes_injected > 0 || memory.constraints_learned > 0 ? "active" : "idle"}
          hint={memory.episodes_injected === 0 && memory.constraints_learned === 0 ? "no reuse" : ""}
          color={memory.episodes_injected > 0 || memory.constraints_learned > 0 ? "text-accent-green" : "text-accent-yellow"}
        />
      </div>
    </div>
  );
}
