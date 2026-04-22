import * as Tooltip from "@radix-ui/react-tooltip";
import type { IntegrityReport } from "../types";

interface Props {
  integrity: IntegrityReport;
}

export function IntegrityBadge({ integrity }: Props) {
  let label: string;
  let className: string;
  if (integrity.confidence >= 0.95) {
    label = "Complete";
    className =
      "bg-accent-green/10 text-accent-green border-accent-green/40";
  } else if (integrity.confidence >= 0.7) {
    label = `Partial (${(integrity.confidence * 100).toFixed(0)}%)`;
    className =
      "bg-accent-yellow/10 text-accent-yellow border-accent-yellow/40";
  } else {
    label = `Degraded (${(integrity.confidence * 100).toFixed(0)}%)`;
    className = "bg-accent-red/10 text-accent-red border-accent-red/40";
  }
  return (
    <Tooltip.Provider delayDuration={150}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>
          <span
            data-testid="integrity-badge"
            className={`rounded-full border px-2 py-[2px] text-[11px] font-medium cursor-help ${className}`}
          >
            {label}
          </span>
        </Tooltip.Trigger>
        <Tooltip.Content
          sideOffset={6}
          className="max-w-[320px] rounded-lg bg-surface-4 border border-border px-3 py-2 text-[12px] text-text-1 shadow-lg"
        >
          <div>
            Expected events: {integrity.total_events_expected}
          </div>
          <div>Received: {integrity.total_events_received}</div>
          <div>Gaps: {integrity.missing_sequences.length}</div>
          <div>Drop sentinels: {integrity.drop_sentinels_found}</div>
          <div>Writer recoveries: {integrity.writer_recoveries_found}</div>
          <div>Schema v{integrity.schema_version}</div>
          <Tooltip.Arrow className="fill-surface-4" />
        </Tooltip.Content>
      </Tooltip.Root>
    </Tooltip.Provider>
  );
}
