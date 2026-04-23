import type { ContextHealthMetrics } from "../types";

interface Props {
  context: ContextHealthMetrics;
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

function pct(n: number): string {
  return `${(n * 100).toFixed(1)}%`;
}

function fmt(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return n.toFixed(0);
}

export function ContextHealthPanel({ context }: Props) {
  return (
    <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="context-health">
      <h3 className="text-[13px] font-semibold text-text-1 mb-3">Context Health</h3>
      <div className="grid grid-cols-4 gap-3">
        <Stat label="Avg size" value={fmt(context.avg_context_size_tokens)} hint="tokens/iter" />
        <Stat label="Max size" value={fmt(context.max_context_size_tokens)} hint="peak" />
        <Stat
          label="Growth rate"
          value={`${context.context_growth_rate >= 0 ? "+" : ""}${context.context_growth_rate.toFixed(0)}`}
          hint="tok/iter"
          color={context.context_growth_rate > 500 ? "text-accent-red" : context.context_growth_rate > 100 ? "text-accent-yellow" : "text-accent-green"}
        />
        <Stat
          label="Compactions"
          value={context.compaction_count.toString()}
          hint={context.compaction_count > 0 ? `${pct(context.compaction_savings_ratio)} saved` : undefined}
          color={context.compaction_count > 0 ? "text-accent-yellow" : "text-accent-green"}
        />
        <Stat
          label="Refetch rate"
          value={pct(context.refetch_rate)}
          hint="re-reads"
          color={context.refetch_rate > 0.2 ? "text-accent-red" : context.refetch_rate > 0.1 ? "text-accent-yellow" : "text-accent-green"}
        />
        <Stat
          label="Action repeat"
          value={pct(context.action_repetition_rate)}
          hint="dup ops"
          color={context.action_repetition_rate > 0.2 ? "text-accent-red" : context.action_repetition_rate > 0.1 ? "text-accent-yellow" : "text-accent-green"}
        />
        <Stat
          label="Usefulness"
          value={pct(context.usefulness_avg)}
          hint="block use"
          color={context.usefulness_avg > 0.5 ? "text-accent-green" : context.usefulness_avg > 0.2 ? "text-accent-yellow" : "text-accent-red"}
        />
        <Stat
          label="Savings"
          value={pct(context.compaction_savings_ratio)}
          hint="per compaction"
        />
      </div>
    </div>
  );
}
