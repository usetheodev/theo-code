import * as Tooltip from "@radix-ui/react-tooltip";
import type { SurrogateMetric } from "../types";

interface MetricCardProps {
  label: string;
  metric: SurrogateMetric;
  format?: (value: number) => string;
  /** Thresholds used to color the value: [good, warning]. Lower=good by default. */
  thresholds?: { good: number; warning: number; lowerIsBetter?: boolean };
}

function defaultFormat(v: number): string {
  if (Math.abs(v) >= 1000) return Math.round(v).toString();
  return v.toFixed(2);
}

function colorFor(value: number, t?: MetricCardProps["thresholds"]): string {
  if (!t) return "bg-accent-blue/10 text-accent-blue";
  const lowerIsBetter = t.lowerIsBetter ?? true;
  const good = lowerIsBetter ? value <= t.good : value >= t.good;
  const warning = lowerIsBetter ? value <= t.warning : value >= t.warning;
  if (good) return "bg-accent-green/10 text-accent-green";
  if (warning) return "bg-accent-yellow/10 text-accent-yellow";
  return "bg-accent-red/10 text-accent-red";
}

function confidenceClass(confidence: number): string {
  if (confidence >= 0.5) return "border-border";
  if (confidence >= 0.2) return "border-accent-yellow/60";
  return "border-accent-red/60";
}

export function MetricCard({
  label,
  metric,
  format = defaultFormat,
  thresholds,
}: MetricCardProps) {
  const color = colorFor(metric.value, thresholds);
  const border = confidenceClass(metric.confidence);
  const widthPct = Math.max(0, Math.min(1, metric.value)) * 100;
  return (
    <Tooltip.Provider delayDuration={150}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>
          <div
            data-testid={`metric-card-${label}`}
            className={`rounded-xl border ${border} bg-surface-2 p-4 flex flex-col gap-2 min-w-[140px] cursor-help transition-colors`}
          >
            <span className="text-[11px] uppercase tracking-wide text-text-3">
              {label}
            </span>
            <span className={`text-[22px] font-semibold ${color}`}>
              {format(metric.value)}
            </span>
            <div className="h-1 rounded bg-surface-3 overflow-hidden">
              <div
                className={color.split(" ")[0].replace("/10", "/60") + " h-1"}
                style={{ width: `${widthPct}%` }}
              />
            </div>
            <span
              className={`text-[10px] ${
                metric.confidence < 0.2
                  ? "text-accent-red"
                  : metric.confidence < 0.5
                    ? "text-accent-yellow"
                    : "text-text-3"
              }`}
            >
              conf: {(metric.confidence * 100).toFixed(0)}%
            </span>
          </div>
        </Tooltip.Trigger>
        <Tooltip.Content
          sideOffset={6}
          className="max-w-[320px] rounded-lg bg-surface-4 border border-border px-3 py-2 text-[12px] text-text-1 shadow-lg"
        >
          <strong>Caveat:</strong> {metric.caveat || "—"}
          <div className="mt-1 text-text-3 text-[10px]">
            {metric.numerator.toFixed(0)} / {metric.denominator.toFixed(0)}
          </div>
          <Tooltip.Arrow className="fill-surface-4" />
        </Tooltip.Content>
      </Tooltip.Root>
    </Tooltip.Provider>
  );
}
