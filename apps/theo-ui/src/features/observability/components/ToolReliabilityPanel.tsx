import type { ToolBreakdown } from "../types";

interface Props {
  tools: ToolBreakdown[];
}

function fmt(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}s`;
  return `${n.toFixed(0)}ms`;
}

export function ToolReliabilityPanel({ tools }: Props) {
  if (tools.length === 0) {
    return (
      <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="tool-reliability">
        <h3 className="text-[13px] font-semibold text-text-1 mb-2">Tool Reliability</h3>
        <div className="text-text-3 text-[12px]">No tool calls in this run.</div>
      </div>
    );
  }
  const sorted = [...tools].sort((a, b) => b.call_count - a.call_count);
  return (
    <div className="bg-surface-2 rounded-xl border border-border p-4" data-testid="tool-reliability">
      <h3 className="text-[13px] font-semibold text-text-1 mb-3">Tool Reliability</h3>
      <table className="w-full text-[12px]">
        <thead>
          <tr className="text-text-3 border-b border-border">
            <th className="text-left py-1">Tool</th>
            <th className="text-right py-1">Calls</th>
            <th className="text-right py-1">✓</th>
            <th className="text-right py-1">✗</th>
            <th className="text-right py-1">Success</th>
            <th className="text-right py-1">Retry</th>
            <th className="text-right py-1">Avg</th>
            <th className="text-right py-1">Max</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((t) => {
            const sr = t.success_rate;
            const srColor = sr >= 0.95 ? "text-accent-green" : sr >= 0.7 ? "text-accent-yellow" : "text-accent-red";
            return (
              <tr key={t.tool_name} className="border-b border-border/30 hover:bg-surface-3">
                <td className="py-1 text-text-1 font-mono">{t.tool_name}</td>
                <td className="py-1 text-right font-mono">{t.call_count}</td>
                <td className="py-1 text-right font-mono text-accent-green">{t.success_count}</td>
                <td className="py-1 text-right font-mono text-accent-red">{t.failure_count}</td>
                <td className={`py-1 text-right font-mono font-semibold ${srColor}`}>
                  {(sr * 100).toFixed(0)}%
                </td>
                <td className="py-1 text-right font-mono">{t.retry_count}</td>
                <td className="py-1 text-right font-mono">{fmt(t.avg_latency_ms)}</td>
                <td className="py-1 text-right font-mono">{fmt(t.max_latency_ms)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
