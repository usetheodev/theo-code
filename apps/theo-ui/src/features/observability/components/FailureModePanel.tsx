import type { ProjectedStep } from "../types";

interface Props {
  steps: ProjectedStep[];
}

interface Mode {
  id: string;
  label: string;
  detector: (steps: ProjectedStep[]) => boolean;
  description: string;
}

const MODES: Mode[] = [
  {
    id: "FM-1",
    label: "NoProgressLoop",
    detector: detectNoProgress,
    description: "Multiple loops without any file edits.",
  },
  {
    id: "FM-2",
    label: "RepeatedSameError",
    detector: detectRepeatedError,
    description: "Same error surfaced repeatedly.",
  },
  {
    id: "FM-3",
    label: "PrematureTermination",
    detector: detectPremature,
    description: "Agent converged without producing edits.",
  },
  {
    id: "FM-4",
    label: "WeakVerification",
    detector: detectWeakVerify,
    description: "Edits made without subsequent verification.",
  },
  {
    id: "FM-5",
    label: "TaskDerailment",
    detector: () => false,
    description: "5+ tool calls ignoring initial-context files.",
  },
  {
    id: "FM-6",
    label: "HistoryLoss",
    detector: () => false,
    description: "Pre-compaction hot file re-read after overflow.",
  },
];

function hasToolOutcome(
  s: ProjectedStep,
  outcome: "Success" | "Failure",
): boolean {
  if (s.event_type !== "ToolCallCompleted") return false;
  if (outcome === "Success") return s.outcome === "Success";
  if (s.outcome && typeof s.outcome === "object" && "Failure" in s.outcome)
    return true;
  return false;
}

function detectNoProgress(steps: ProjectedStep[]): boolean {
  const edits = steps.filter(
    (s) =>
      hasToolOutcome(s, "Success") &&
      (s.tool_name === "edit_file" || s.tool_name === "write_file"),
  ).length;
  const iter = steps.filter((s) => s.event_type === "LlmCallStart").length;
  return iter >= 4 && edits === 0;
}

function detectRepeatedError(steps: ProjectedStep[]): boolean {
  const errors = steps.filter((s) => s.event_type === "Error");
  if (errors.length < 3) return false;
  const first = errors[0].payload_summary;
  return errors.slice(1).every((e) => e.payload_summary === first);
}

function detectPremature(steps: ProjectedStep[]): boolean {
  const converged = steps.some(
    (s) =>
      s.event_type === "RunStateChanged" &&
      s.payload_summary.toLowerCase().includes("converged"),
  );
  const edits = steps.filter(
    (s) =>
      hasToolOutcome(s, "Success") &&
      (s.tool_name === "edit_file" || s.tool_name === "write_file"),
  ).length;
  const budgetExceeded = steps.some(
    (s) => s.event_type === "BudgetExceeded",
  );
  const iter = steps.filter((s) => s.event_type === "LlmCallStart").length;
  return converged && edits === 0 && iter >= 2 && !budgetExceeded;
}

function detectWeakVerify(steps: ProjectedStep[]): boolean {
  for (let i = 0; i < steps.length; i++) {
    const s = steps[i];
    if (
      hasToolOutcome(s, "Success") &&
      (s.tool_name === "edit_file" || s.tool_name === "write_file")
    ) {
      const win = steps.slice(i + 1, Math.min(steps.length, i + 4));
      const hasVerify = win.some(
        (x) =>
          (x.event_type === "ToolCallCompleted" && x.tool_name === "bash") ||
          x.event_type === "SensorExecuted",
      );
      if (!hasVerify) return true;
    }
  }
  return false;
}

export function FailureModePanel({ steps }: Props) {
  return (
    <div
      className="flex flex-col gap-1 bg-surface-2 rounded-xl border border-border p-4"
      data-testid="failure-panel"
    >
      <h3 className="text-[13px] font-semibold text-text-1 mb-2">
        Failure Analysis
      </h3>
      {MODES.map((m) => {
        const detected = m.detector(steps);
        return (
          <div
            key={m.id}
            className="flex items-center gap-3 py-1"
            data-testid={`failure-row-${m.id}`}
          >
            <span
              className={`text-[13px] font-semibold ${
                detected ? "text-accent-red" : "text-accent-green"
              }`}
            >
              {detected ? "⚠" : "✓"}
            </span>
            <span className="text-[13px] text-text-1 w-40">{m.label}</span>
            <span className="text-[12px] text-text-3 flex-1">
              {detected ? m.description : "Not detected"}
            </span>
          </div>
        );
      })}
    </div>
  );
}
