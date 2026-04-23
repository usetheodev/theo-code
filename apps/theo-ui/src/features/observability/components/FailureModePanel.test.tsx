import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { FailureModePanel } from "./FailureModePanel";
import type { ProjectedStep } from "../types";

function mk(seq: number, et: string, overrides: Partial<ProjectedStep> = {}): ProjectedStep {
  return {
    sequence: seq,
    event_type: et,
    event_kind: "Lifecycle",
    timestamp: seq,
    entity_id: `e${seq}`,
    payload_summary: "",
    duration_ms: null,
    tool_name: null,
    outcome: null,
    ...overrides,
  };
}

describe("FailureModePanel", () => {
  it("test_failure_panel_shows_6_modes", () => {
    const { container } = render(<FailureModePanel steps={[]} />);
    const rows = container.querySelectorAll('[data-testid^="failure-row-"]');
    expect(rows.length).toBe(6);
  });

  it("test_failure_panel_highlights_detected_modes", () => {
    // Premature: converged with 0 edits + >=2 LlmCallStart.
    render(
      <FailureModePanel
        steps={[
          mk(0, "LlmCallStart"),
          mk(1, "LlmCallStart"),
          mk(2, "RunStateChanged", { payload_summary: "converged" }),
        ]}
      />,
    );
    expect(screen.getByText(/Agent converged without producing edits/)).toBeTruthy();
  });

  it("test_failure_panel_shows_description_for_detected", () => {
    // Detect weak verification: edit followed by no verification.
    render(
      <FailureModePanel
        steps={[
          mk(0, "ToolCallCompleted", { tool_name: "edit_file", outcome: "Success" }),
          mk(1, "LlmCallStart"),
        ]}
      />,
    );
    expect(screen.getByText(/Edits made without subsequent verification/)).toBeTruthy();
  });
});
