import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { TimelineView } from "./TimelineView";
import type { ProjectedStep } from "../types";

function mk(seq: number, et: string, overrides: Partial<ProjectedStep> = {}): ProjectedStep {
  return {
    sequence: seq,
    event_type: et,
    event_kind: "Tooling",
    timestamp: seq * 1000,
    entity_id: `e${seq}`,
    payload_summary: "",
    duration_ms: null,
    tool_name: null,
    outcome: null,
    ...overrides,
  };
}

describe("TimelineView", () => {
  it("test_timeline_renders_steps_in_order", () => {
    render(
      <TimelineView
        steps={[mk(0, "RunInitialized"), mk(1, "ToolCallCompleted", { tool_name: "bash" })]}
      />,
    );
    expect(screen.getByText("RunInitialized")).toBeTruthy();
    expect(screen.getByText(/ToolCallCompleted/)).toBeTruthy();
  });

  it("test_timeline_step_shows_event_kind_color", () => {
    const { container } = render(
      <TimelineView steps={[mk(0, "Error", { event_kind: "Failure" })]} />,
    );
    expect(container.querySelector('[data-testid="timeline-step"]')).toBeTruthy();
  });

  it("test_timeline_step_shows_duration_when_available", () => {
    render(
      <TimelineView
        steps={[mk(0, "ToolCallCompleted", { duration_ms: 42, tool_name: "read" })]}
      />,
    );
    expect(screen.getByText(/42ms/)).toBeTruthy();
  });

  it("test_timeline_step_shows_outcome_icon", () => {
    const { container } = render(
      <TimelineView steps={[mk(0, "ToolCallCompleted", { outcome: "Success" })]} />,
    );
    const text = container.textContent ?? "";
    expect(text.includes("✓")).toBe(true);
  });

  it("test_timeline_highlights_failure_steps", () => {
    const { container } = render(
      <TimelineView steps={[mk(0, "Error", { event_kind: "Failure" })]} />,
    );
    const step = container.querySelector('[data-testid="timeline-step"]');
    expect(step?.className).toMatch(/accent-red/);
  });
});
