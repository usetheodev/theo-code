import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ToolUsageChart } from "./ToolUsageChart";
import type { ProjectedStep } from "../types";

function mk(seq: number, tool: string, success: boolean): ProjectedStep {
  return {
    sequence: seq,
    event_type: "ToolCallCompleted",
    event_kind: "Tooling",
    timestamp: seq,
    entity_id: `e${seq}`,
    payload_summary: "",
    duration_ms: null,
    tool_name: tool,
    outcome: success ? "Success" : { Failure: { retryable: false } },
  };
}

describe("ToolUsageChart", () => {
  it("test_tool_chart_renders_bars_per_tool", () => {
    render(<ToolUsageChart steps={[mk(0, "a", true), mk(1, "b", true)]} />);
    expect(screen.getByTestId("tool-row-a")).toBeTruthy();
    expect(screen.getByTestId("tool-row-b")).toBeTruthy();
  });

  it("test_tool_chart_shows_success_failure_split", () => {
    render(<ToolUsageChart steps={[mk(0, "a", true), mk(1, "a", false)]} />);
    expect(screen.getByText(/1 ✓, 1 ✗/)).toBeTruthy();
  });

  it("test_tool_chart_sorted_by_count_descending", () => {
    const { container } = render(
      <ToolUsageChart
        steps={[mk(0, "a", true), mk(1, "b", true), mk(2, "b", true)]}
      />,
    );
    const rows = container.querySelectorAll('[data-testid^="tool-row-"]');
    expect(rows[0].getAttribute("data-testid")).toBe("tool-row-b");
  });

  it("test_tool_chart_empty_when_no_tool_calls", () => {
    render(<ToolUsageChart steps={[]} />);
    expect(screen.getByText(/No tool calls/)).toBeTruthy();
  });
});
