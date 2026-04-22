import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MetricCard } from "./MetricCard";
import type { SurrogateMetric } from "../types";

const baseMetric: SurrogateMetric = {
  value: 0.42,
  confidence: 0.8,
  numerator: 4,
  denominator: 10,
  is_surrogate: true,
  caveat: "Proxy metric for testing.",
};

describe("MetricCard", () => {
  it("test_metric_card_renders_value", () => {
    render(<MetricCard label="Doom" metric={baseMetric} />);
    expect(screen.getByText("0.42")).toBeTruthy();
  });

  it("test_metric_card_shows_confidence_badge", () => {
    render(<MetricCard label="Doom" metric={baseMetric} />);
    expect(screen.getByText(/conf:\s*80%/)).toBeTruthy();
  });

  it("test_metric_card_tooltip_shows_caveat", () => {
    const { container } = render(<MetricCard label="Doom" metric={baseMetric} />);
    const trigger = container.querySelector('[data-testid="metric-card-Doom"]');
    expect(trigger).toBeTruthy();
  });
});
