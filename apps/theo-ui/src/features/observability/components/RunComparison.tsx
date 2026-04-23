import type { DerivedMetrics, RunSummary } from "../types";

interface Props {
  runs: RunSummary[];
  metrics: DerivedMetrics[];
}

function delta(a: number, b: number): {
  pct: number;
  direction: "up" | "down" | "flat";
} {
  if (a === 0 && b === 0) return { pct: 0, direction: "flat" };
  if (a === 0) return { pct: Infinity, direction: "up" };
  const pct = ((b - a) / Math.abs(a)) * 100;
  const direction = pct > 0 ? "up" : pct < 0 ? "down" : "flat";
  return { pct, direction };
}

const ROWS: Array<{
  key: keyof DerivedMetrics;
  label: string;
  /** true = higher value is a regression, false = higher is better. */
  lowerIsBetter: boolean;
}> = [
  { key: "doom_loop_frequency", label: "Doom Loop Freq", lowerIsBetter: true },
  { key: "llm_efficiency", label: "LLM Efficiency", lowerIsBetter: false },
  { key: "context_waste_ratio", label: "Context Waste", lowerIsBetter: true },
  { key: "hypothesis_churn_rate", label: "Hypothesis Churn", lowerIsBetter: true },
  { key: "time_to_first_tool_ms", label: "Time to 1st Tool", lowerIsBetter: true },
];

export function RunComparison({ runs, metrics }: Props) {
  if (runs.length === 0) return null;
  const baseline = metrics[0];
  return (
    <table
      className="w-full text-[12px] border-separate border-spacing-y-1"
      data-testid="run-comparison"
    >
      <thead>
        <tr className="text-text-3">
          <th className="text-left py-1 pr-2">Metric</th>
          {runs.map((r) => (
            <th key={r.run_id} className="text-right px-2">
              {r.run_id.slice(0, 8)}
            </th>
          ))}
          {runs.length > 1 && (
            <th className="text-right pl-2">Δ vs. first</th>
          )}
        </tr>
      </thead>
      <tbody>
        {ROWS.map((row) => (
          <tr key={row.key}>
            <td className="text-text-2 pr-2">{row.label}</td>
            {metrics.map((m, i) => (
              <td
                key={runs[i].run_id}
                className="text-right px-2 text-text-1 font-mono"
              >
                {m[row.key].value.toFixed(2)}
              </td>
            ))}
            {runs.length > 1 && metrics[metrics.length - 1] && (
              <td className="text-right pl-2">
                {(() => {
                  const a = baseline[row.key].value;
                  const b = metrics[metrics.length - 1][row.key].value;
                  const d = delta(a, b);
                  const improvement = row.lowerIsBetter
                    ? d.direction === "down"
                    : d.direction === "up";
                  const color = improvement
                    ? "text-accent-green"
                    : d.direction === "flat"
                      ? "text-text-3"
                      : "text-accent-red";
                  const arrow =
                    d.direction === "up" ? "↑" : d.direction === "down" ? "↓" : "→";
                  return (
                    <span className={`font-mono ${color}`}>
                      {isFinite(d.pct) ? `${d.pct.toFixed(0)}% ${arrow}` : "—"}
                    </span>
                  );
                })()}
              </td>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
