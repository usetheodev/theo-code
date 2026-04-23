import { useState } from "react";
import type { EventKind, ProjectedStep } from "../types";

interface Props {
  steps: ProjectedStep[];
}

const KIND_COLORS: Record<EventKind, string> = {
  Lifecycle: "bg-text-3/20 text-text-2",
  Tooling: "bg-accent-blue/20 text-accent-blue",
  Reasoning: "bg-accent-purple/20 text-accent-purple",
  Context: "bg-accent-yellow/20 text-accent-yellow",
  Failure: "bg-accent-red/20 text-accent-red",
  Streaming: "bg-text-3/10 text-text-3",
};

function outcomeIcon(step: ProjectedStep): string {
  if (!step.outcome) return "•";
  if (step.outcome === "Success") return "✓";
  if (step.outcome === "Timeout") return "⧖";
  if (step.outcome === "Skipped") return "⊘";
  return "✗";
}

function formatRelative(baseTs: number, ts: number): string {
  const deltaS = Math.max(0, (ts - baseTs) / 1000);
  const mm = Math.floor(deltaS / 60).toString().padStart(2, "0");
  const ss = Math.floor(deltaS % 60).toString().padStart(2, "0");
  return `${mm}:${ss}`;
}

export function TimelineView({ steps }: Props) {
  const [expanded, setExpanded] = useState<number | null>(null);
  if (steps.length === 0) {
    return (
      <div className="text-text-3 text-center text-[13px] py-8">
        No steps captured in this run.
      </div>
    );
  }
  const baseTs = steps[0].timestamp;
  return (
    <ol className="flex flex-col gap-1" data-testid="timeline-view">
      {steps.map((s, idx) => {
        const kind = (s.event_kind ?? "Lifecycle") as EventKind;
        const badge = KIND_COLORS[kind] ?? "bg-surface-3 text-text-3";
        const isFailure = kind === "Failure";
        return (
          <li
            key={s.sequence}
            data-testid="timeline-step"
            className={`flex items-start gap-3 rounded-md px-3 py-2 border cursor-pointer ${
              isFailure ? "border-accent-red/30" : "border-transparent"
            } hover:bg-surface-3`}
            onClick={() => setExpanded(expanded === idx ? null : idx)}
          >
            <span className="font-mono text-[11px] text-text-3 w-12 shrink-0">
              {formatRelative(baseTs, s.timestamp)}
            </span>
            <span className="text-[14px] shrink-0 text-text-2">
              {outcomeIcon(s)}
            </span>
            <span className="text-[13px] text-text-1 flex-1">
              {s.event_type}
              {s.tool_name && (
                <span className="ml-1 text-text-3">: {s.tool_name}</span>
              )}
              {s.duration_ms !== null && s.duration_ms !== undefined && (
                <span className="ml-1 text-text-3 text-[11px]">
                  ({s.duration_ms}ms)
                </span>
              )}
            </span>
            <span
              className={`px-2 py-[2px] rounded text-[10px] uppercase ${badge}`}
            >
              {kind}
            </span>
            {expanded === idx && (
              <pre className="basis-full text-[11px] text-text-2 font-mono mt-1 whitespace-pre-wrap break-words">
                {s.payload_summary}
              </pre>
            )}
          </li>
        );
      })}
    </ol>
  );
}
