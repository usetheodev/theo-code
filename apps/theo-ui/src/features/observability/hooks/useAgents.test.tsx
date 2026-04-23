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

  it("selectAgent fetches the detail endpoint", async () => {
    fetchMock.mockResolvedValueOnce({
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
    });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.selectAgent("explorer");
    });
    expect(result.current.selected?.stats.agent_name).toBe("explorer");
    expect(result.current.selected?.recent_runs.length).toBe(1);
    expect(fetchMock).toHaveBeenCalledWith("/api/agents/explorer");
  });

  it("selectAgent URL-encodes the agent name", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => ({ stats: sampleStats, recent_runs: [] }),
    });
    const { result } = renderHook(() => useAgents());
    await act(async () => {
      await result.current.selectAgent("agent with space");
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/agents/agent%20with%20space",
    );
  });

  it("clearSelection resets selected to null", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => ({ stats: sampleStats, recent_runs: [] }),
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
  });
});
