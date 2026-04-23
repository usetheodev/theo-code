import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// Simulate non-Tauri environment: force fetch-based path.
(globalThis as Record<string, unknown>).window = (globalThis as Record<string, unknown>).window ?? {};

const fetchMock = vi.fn();
(globalThis as Record<string, unknown>).fetch = (...args: unknown[]) => fetchMock(...(args as [string, RequestInit?]));

import { useObservability } from "./useObservability";

describe("useObservability", () => {
  beforeEach(() => {
    fetchMock.mockReset();
  });

  it("test_useObservability_starts_with_empty_state", () => {
    const { result } = renderHook(() => useObservability());
    expect(result.current.runs).toEqual([]);
    expect(result.current.selectedRun).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("test_loadRuns_populates_runs_array", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => [
        {
          run_id: "run-1",
          timestamp: 0,
          success: true,
          total_steps: 1,
          total_tool_calls: 0,
          duration_ms: 0,
          metrics: {},
        },
      ],
    });
    const { result } = renderHook(() => useObservability());
    await act(async () => {
      await result.current.loadRuns();
    });
    expect(result.current.runs.length).toBe(1);
    expect(result.current.runs[0].run_id).toBe("run-1");
    expect(fetchMock).toHaveBeenCalledWith("/api/list_runs");
  });

  it("test_selectRun_fetches_trajectory", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => ({
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
      }),
    });
    const { result } = renderHook(() => useObservability());
    await act(async () => {
      await result.current.selectRun("r");
    });
    expect(result.current.selectedRun).toBeTruthy();
    expect(result.current.selectedRun?.run_id).toBe("r");
    expect(fetchMock).toHaveBeenCalledWith("/api/run/r/trajectory");
  });

  it("test_error_state_on_invoke_failure", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 500,
      json: async () => ({}),
    });
    const { result } = renderHook(() => useObservability());
    await act(async () => {
      await result.current.loadRuns();
    });
    expect(result.current.error).toContain("list_runs failed");
  });
});
