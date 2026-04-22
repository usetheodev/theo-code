import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// Mock Tauri invoke before importing the hook.
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

import { useObservability } from "./useObservability";

describe("useObservability", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("test_useObservability_starts_with_empty_state", () => {
    const { result } = renderHook(() => useObservability());
    expect(result.current.runs).toEqual([]);
    expect(result.current.selectedRun).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("test_loadRuns_populates_runs_array", async () => {
    invokeMock.mockResolvedValueOnce([
      {
        run_id: "run-1",
        timestamp: 0,
        success: true,
        total_steps: 1,
        total_tool_calls: 0,
        duration_ms: 0,
        metrics: {},
      },
    ]);
    const { result } = renderHook(() => useObservability());
    await act(async () => {
      await result.current.loadRuns();
    });
    expect(result.current.runs.length).toBe(1);
    expect(result.current.runs[0].run_id).toBe("run-1");
  });

  it("test_selectRun_fetches_trajectory", async () => {
    invokeMock.mockResolvedValueOnce({
      run_id: "r",
      trajectory_id: "t",
      steps: [],
      integrity: {
        complete: true,
        total_events_expected: 0,
        total_events_received: 0,
        missing_sequences: [],
        drop_sentinels_found: 0,
        writer_recoveries_found: 0,
        confidence: 1.0,
        schema_version: 1,
      },
    });
    const { result } = renderHook(() => useObservability());
    await act(async () => {
      await result.current.selectRun("r");
    });
    expect(result.current.selectedRun).toBeTruthy();
    expect(result.current.selectedRun?.run_id).toBe("r");
  });

  it("test_error_state_on_invoke_failure", async () => {
    invokeMock.mockRejectedValueOnce("boom");
    const { result } = renderHook(() => useObservability());
    await act(async () => {
      await result.current.loadRuns();
    });
    expect(result.current.error).toContain("boom");
  });
});
