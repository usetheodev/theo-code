import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

import { AgentMetricsCard } from "./AgentMetricsCard";
import type { AgentStats } from "../hooks/useAgents";

const baseStats: AgentStats = {
  agent_name: "explorer",
  agent_source: "builtin",
  run_count: 5,
  success_count: 4,
  failure_count: 1,
  cancelled_count: 0,
  abandoned_count: 0,
  running_count: 0,
  total_tokens_used: 1000,
  total_iterations_used: 25,
  avg_tokens_per_run: 200,
  avg_iterations_per_run: 5,
  success_rate: 0.8,
  last_started_at: 1_700_000_000,
};

describe("AgentMetricsCard", () => {
  it("renders the agent name and source label", () => {
    render(<AgentMetricsCard stats={baseStats} />);
    expect(screen.getByText("explorer")).toBeTruthy();
    expect(screen.getByText("Built-in")).toBeTruthy();
  });

  it("renders the run count and success rate as percent", () => {
    render(<AgentMetricsCard stats={baseStats} />);
    expect(screen.getByText("5")).toBeTruthy();
    expect(screen.getByText("80%")).toBeTruthy();
  });

  it("calls onSelect when clicked", () => {
    const onSelect = vi.fn();
    render(<AgentMetricsCard stats={baseStats} onSelect={onSelect} />);
    fireEvent.click(screen.getByTestId("agent-card-explorer"));
    expect(onSelect).toHaveBeenCalledWith("explorer");
  });

  it("does not error when no onSelect is provided", () => {
    render(<AgentMetricsCard stats={baseStats} />);
    fireEvent.click(screen.getByTestId("agent-card-explorer"));
    // No assertion needed — absence of throw is the assertion.
  });

  it("renders failure / cancelled badges when counts > 0", () => {
    const stats = {
      ...baseStats,
      failure_count: 2,
      cancelled_count: 1,
      abandoned_count: 1,
      running_count: 3,
    };
    render(<AgentMetricsCard stats={stats} />);
    expect(screen.getByText(/3 running/)).toBeTruthy();
    expect(screen.getByText(/2 failed/)).toBeTruthy();
    expect(screen.getByText(/1 cancelled/)).toBeTruthy();
    expect(screen.getByText(/1 abandoned/)).toBeTruthy();
  });

  it("hides badges when counts are zero", () => {
    render(<AgentMetricsCard stats={baseStats} />);
    expect(screen.queryByText(/running/)).toBeNull();
    expect(screen.queryByText(/cancelled/)).toBeNull();
  });

  it("supports keyboard activation via Enter", () => {
    const onSelect = vi.fn();
    render(<AgentMetricsCard stats={baseStats} onSelect={onSelect} />);
    fireEvent.keyDown(screen.getByTestId("agent-card-explorer"), {
      key: "Enter",
    });
    expect(onSelect).toHaveBeenCalledWith("explorer");
  });

  it("applies the selected styles when selected=true", () => {
    render(<AgentMetricsCard stats={baseStats} onSelect={() => {}} selected />);
    const card = screen.getByTestId("agent-card-explorer");
    expect(card.className).toContain("border-accent-blue");
  });
});
