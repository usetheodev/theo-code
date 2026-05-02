import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

(globalThis as Record<string, unknown>).window =
  (globalThis as Record<string, unknown>).window ?? {};

const fetchMock = vi.fn();
(globalThis as Record<string, unknown>).fetch = (...args: unknown[]) =>
  fetchMock(...(args as [string, RequestInit?]));

import { useAgents } from "./useAgents";

const sampleStats = {
  agent_name: "explorer",
  agent_source: "builtin",
  run_count: 3,
  success_count: 2,
  failure_count: 1,
  cancelled_count: 0,
  abandoned_count: 0,
  running_count: 0,
  total_tokens_used: 600,
  total_iterations_used: 12,
  avg_tokens_per_run: 200,
  avg_iterations_per_run: 4,
  success_rate: 2 / 3,
  last_started_at: 1_700_000_000,
};

describe("useAgents", () => {
  beforeEach(() => {
    fetchMock.mockReset();
  });

  it("starts with empty state", () => {
    const { result } = renderHook(() => useAgents());
    expect(result.current.agents).toEqual([]);
    expect(result.current.selected).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("loadAgents populates the agents array", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => [sampleStats],
    });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.loadAgents();
    });
    expect(result.current.agents.length).toBe(1);
    expect(result.current.agents[0].agent_name).toBe("explorer");
    expect(fetchMock).toHaveBeenCalledWith("/api/agents");
  });

  it("loadAgents records error on non-ok response", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 500,
      json: async () => ({}),
    });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.loadAgents();
    });
    expect(result.current.agents.length).toBe(0);
    expect(result.current.error).toContain("500");
  });

  it("selectAgent fetches the detail endpoint and runs endpoint", async () => {
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({
          stats: sampleStats,
          recent_runs: [
            {
              run_id: "r-1",
              status: "completed",
              started_at: 1_700_000_000,
              finished_at: 1_700_000_010,
              iterations_used: 3,
              tokens_used: 150,
              objective: "obj",
              summary: "ok",
            },
          ],
        }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => [],
      });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.selectAgent("explorer");
    });
    expect(result.current.selected?.stats.agent_name).toBe("explorer");
    expect(result.current.selected?.recent_runs.length).toBe(1);
    expect(fetchMock).toHaveBeenCalledWith("/api/agents/explorer");
    expect(fetchMock).toHaveBeenCalledWith("/api/agents/explorer/runs");
  });

  it("selectAgent URL-encodes the agent name on both endpoints", async () => {
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ stats: sampleStats, recent_runs: [] }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => [],
      });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.selectAgent("agent with space");
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/agents/agent%20with%20space",
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/agents/agent%20with%20space/runs",
    );
  });

  it("selectAgent populates selectedRuns from /runs endpoint", async () => {
    const sampleRun = {
      run_id: "r-1",
      status: "completed",
      started_at: 100,
      finished_at: 110,
      iterations_used: 2,
      tokens_used: 50,
      objective: "do x",
      summary: null,
    };
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ stats: sampleStats, recent_runs: [] }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => [sampleRun, sampleRun],
      });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.selectAgent("explorer");
    });
    expect(result.current.selectedRuns.length).toBe(2);
    expect(result.current.selectedRuns[0].run_id).toBe("r-1");
  });

  it("clearSelection resets selected to null and selectedRuns to []", async () => {
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ stats: sampleStats, recent_runs: [] }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => [],
      });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.selectAgent("explorer");
    });
    expect(result.current.selected).not.toBeNull();
    act(() => {
      result.current.clearSelection();
    });
    expect(result.current.selected).toBeNull();
    expect(result.current.selectedRuns).toEqual([]);
  });

  it("liveEvents starts as an empty array", () => {
    const { result } = renderHook(() => useAgents());
    expect(result.current.liveEvents).toEqual([]);
  });
});
