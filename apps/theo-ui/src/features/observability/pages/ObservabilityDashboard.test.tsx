import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

import { ObservabilityDashboard } from "./ObservabilityDashboard";

describe("ObservabilityDashboard", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("test_dashboard_shows_empty_state_when_no_runs", async () => {
    invokeMock.mockResolvedValue([]);
    render(<ObservabilityDashboard />);
    await waitFor(() => {
      expect(screen.getByText(/No runs yet/)).toBeTruthy();
    });
  });

  it("test_dashboard_renders_5_cards", async () => {
    invokeMock.mockImplementationOnce(() =>
      Promise.resolve([
        {
          run_id: "r1",
          timestamp: 0,
          success: true,
          total_steps: 0,
          total_tool_calls: 0,
          duration_ms: 0,
          metrics: {
            doom_loop_frequency: {
              value: 0.1,
              confidence: 1,
              numerator: 1,
              denominator: 10,
              is_surrogate: true,
              caveat: "",
            },
            llm_efficiency: {
              value: 0.9,
              confidence: 1,
              numerator: 9,
              denominator: 10,
              is_surrogate: true,
              caveat: "",
            },
            context_waste_ratio: {
              value: 0.05,
              confidence: 1,
              numerator: 1,
              denominator: 20,
              is_surrogate: true,
              caveat: "",
            },
            hypothesis_churn_rate: {
              value: 0.3,
              confidence: 1,
              numerator: 3,
              denominator: 10,
              is_surrogate: true,
              caveat: "",
            },
            time_to_first_tool_ms: {
              value: 1200,
              confidence: 1,
              numerator: 1200,
              denominator: 1,
              is_surrogate: true,
              caveat: "",
            },
          },
        },
      ]),
    );
    invokeMock.mockResolvedValueOnce({
      run_id: "r1",
      trajectory_id: "t",
      steps: [],
      integrity: {
        complete: true,
        total_events_expected: 0,
        total_events_received: 0,
        missing_sequences: [],
        drop_sentinels_found: 0,
        writer_recoveries_found: 0,
        confidence: 1,
        schema_version: 1,
      },
    });
    const { container } = render(<ObservabilityDashboard />);
    await waitFor(() => {
      const cards = container.querySelectorAll('[data-testid^="metric-card-"]');
      expect(cards.length).toBe(5);
    });
  });
});
