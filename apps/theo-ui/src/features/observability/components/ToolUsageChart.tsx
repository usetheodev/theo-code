import { useMemo } from "react";
import type { ProjectedStep, StepOutcome } from "../types";

interface Props {
  steps: ProjectedStep[];
}

interface ToolRow {
  name: string;
  total: number;
  success: number;
  failure: number;
}

function isSuccess(o: StepOutcome | null | undefined): boolean {
  return o === "Success";
}

export function ToolUsageChart({ steps }: Props) {
  const rows = useMemo<ToolRow[]>(() => {
    const map = new Map<string, ToolRow>();
    for (const s of steps) {
      if (s.event_type !== "ToolCallCompleted" || !s.tool_name) continue;
      const row = map.get(s.tool_name) ?? {
        name: s.tool_name,
        total: 0,
        success: 0,
        failure: 0,
      };
      row.total += 1;
      if (isSuccess(s.outcome)) row.success += 1;
      else row.failure += 1;
      map.set(s.tool_name, row);
    }
    return Array.from(map.values()).sort((a, b) => b.total - a.total);
  }, [steps]);

  if (rows.length === 0) {
    return (
      <div className="text-text-3 text-center text-[13px] py-8">
        No tool calls in this run.
      </div>
    );
  }
  const max = Math.max(...rows.map((r) => r.total));
  const totalCalls = rows.reduce((a, r) => a + r.total, 0);

  return (
    <div className="flex flex-col gap-2" data-testid="tool-usage-chart">
      <div className="text-[12px] text-text-3">
        Tool Usage ({totalCalls} calls)
      </div>
      {rows.map((r) => {
        const successWidth = (r.success / max) * 100;
        const failureWidth = (r.failure / max) * 100;
        return (
          <div
            key={r.name}
            className="flex items-center gap-3"
            data-testid={`tool-row-${r.name}`}
          >
            <span className="text-[12px] text-text-1 w-28 shrink-0">
              {r.name}
            </span>
            <div className="flex-1 h-4 bg-surface-3 rounded overflow-hidden flex">
              <div
                className="h-4 bg-accent-green/60"
                style={{ width: `${successWidth}%` }}
                title={`${r.success} success`}
              />
              <div
                className="h-4 bg-accent-red/60"
                style={{ width: `${failureWidth}%` }}
                title={`${r.failure} failures`}
              />
            </div>
            <span className="text-[11px] text-text-3 w-32 text-right">
              {r.total} ({r.success} ✓, {r.failure} ✗)
            </span>
          </div>
        );
      })}
    </div>
  );
}
