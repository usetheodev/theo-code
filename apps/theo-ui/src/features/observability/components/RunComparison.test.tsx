import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { RunComparison } from "./RunComparison";
import type { DerivedMetrics, RunSummary, SurrogateMetric } from "../types";

function metric(value: number): SurrogateMetric {
  return {
    value,
    confidence: 1,
    numerator: value,
    denominator: 1,
    is_surrogate: true,
    caveat: "",
  };
}

function metricsForValue(v: number): DerivedMetrics {
  return {
    doom_loop_frequency: metric(v),
    llm_efficiency: metric(v),
    context_waste_ratio: metric(v),
    hypothesis_churn_rate: metric(v),
    time_to_first_tool_ms: metric(v),
  };
}

function mkRun(id: string, ts: number): RunSummary {
  return {
    run_id: id,
    timestamp: ts,
    success: true,
    total_steps: 10,
    total_tool_calls: 3,
    duration_ms: 1000,
    metrics: metricsForValue(0.5),
  };
}

describe("RunComparison", () => {
  it("test_comparison_shows_delta_percentage", () => {
    render(
      <RunComparison
        runs={[mkRun("a", 1), mkRun("b", 2)]}
        metrics={[metricsForValue(0.5), metricsForValue(0.25)]}
      />,
    );
    // delta from 0.5 → 0.25 = -50%
    expect(screen.getAllByText(/50%/).length).toBeGreaterThan(0);
  });

  it("test_comparison_colors_improvements_green", () => {
    const { container } = render(
      <RunComparison
        runs={[mkRun("a", 1), mkRun("b", 2)]}
        metrics={[metricsForValue(0.5), metricsForValue(0.1)]}
      />,
    );
    // Doom loop lower_is_better → reduction is improvement → green
    expect(container.innerHTML).toMatch(/accent-green/);
  });

  it("test_comparison_colors_regressions_red", () => {
    const { container } = render(
      <RunComparison
        runs={[mkRun("a", 1), mkRun("b", 2)]}
        metrics={[metricsForValue(0.1), metricsForValue(0.5)]}
      />,
    );
    // Doom loop increasing → regression → red
    expect(container.innerHTML).toMatch(/accent-red/);
  });

  it("test_comparison_handles_single_run", () => {
    render(<RunComparison runs={[mkRun("a", 1)]} metrics={[metricsForValue(0.5)]} />);
    // No Δ column for single run
    expect(screen.queryByText(/Δ vs\. first/)).toBeNull();
  });
});
