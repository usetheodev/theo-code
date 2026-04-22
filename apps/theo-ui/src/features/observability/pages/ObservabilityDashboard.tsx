import { useEffect, useState } from "react";
import { Activity } from "lucide-react";
import { useObservability } from "../hooks/useObservability";
import { MetricCard } from "../components/MetricCard";
import { IntegrityBadge } from "../components/IntegrityBadge";
import { TimelineView } from "../components/TimelineView";
import { ToolUsageChart } from "../components/ToolUsageChart";
import { FailureModePanel } from "../components/FailureModePanel";
import { emptyDerivedMetrics } from "../types";

export function ObservabilityDashboard() {
  const { runs, selectedRun, loading, error, loadRuns, selectRun } =
    useObservability();
  const [selectedId, setSelectedId] = useState<string | null>(null);

  useEffect(() => {
    loadRuns();
  }, [loadRuns]);

  useEffect(() => {
    if (!selectedId && runs.length > 0) {
      const first = runs[0].run_id;
      setSelectedId(first);
      selectRun(first);
    }
  }, [runs, selectedId, selectRun]);

  const runMeta = runs.find((r) => r.run_id === selectedId);
  const metrics = runMeta?.metrics ?? emptyDerivedMetrics;

  return (
    <div className="flex-1 flex flex-col">
      <header className="flex items-center justify-between px-6 h-[52px] border-b border-border shrink-0">
        <h2 className="text-[15px] font-semibold text-text-0">
          Agent Observability
        </h2>
        <div className="flex items-center gap-3">
          {selectedRun && <IntegrityBadge integrity={selectedRun.integrity} />}
          <select
            className="bg-surface-2 text-text-1 text-[13px] rounded px-2 py-1 border border-border"
            value={selectedId ?? ""}
            onChange={(e) => {
              const id = e.target.value;
              setSelectedId(id);
              selectRun(id);
            }}
            data-testid="run-selector"
          >
            {runs.map((r) => (
              <option key={r.run_id} value={r.run_id}>
                {r.run_id.slice(0, 12)} · {new Date(r.timestamp).toLocaleString()}
              </option>
            ))}
          </select>
        </div>
      </header>

      {loading && (
        <div className="px-6 py-2 text-[12px] text-text-3">Loading…</div>
      )}
      {error && (
        <div className="px-6 py-2 text-[12px] text-accent-red">{error}</div>
      )}

      {runs.length === 0 && !loading ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="flex flex-col items-center gap-4 text-center animate-fade-in">
            <div className="w-12 h-12 rounded-2xl bg-surface-3 flex items-center justify-center">
              <Activity size={22} className="text-text-3" />
            </div>
            <div>
              <h3 className="text-[14px] font-medium text-text-1 mb-1">
                No runs yet
              </h3>
              <p className="text-[13px] text-text-3 max-w-xs">
                Agent runs will be captured here after the first session.
              </p>
            </div>
          </div>
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto p-6 flex flex-col gap-6">
          <section
            className="grid grid-cols-1 md:grid-cols-5 gap-3"
            data-testid="metric-cards"
          >
            <MetricCard
              label="Doom Loop"
              metric={metrics.doom_loop_frequency}
              thresholds={{ good: 0.1, warning: 0.3, lowerIsBetter: true }}
            />
            <MetricCard
              label="LLM Efficiency"
              metric={metrics.llm_efficiency}
              thresholds={{ good: 0.6, warning: 0.3, lowerIsBetter: false }}
            />
            <MetricCard
              label="Context Waste"
              metric={metrics.context_waste_ratio}
              thresholds={{ good: 0.1, warning: 0.3, lowerIsBetter: true }}
            />
            <MetricCard
              label="Hyp Churn"
              metric={metrics.hypothesis_churn_rate}
              thresholds={{ good: 0.3, warning: 0.6, lowerIsBetter: true }}
            />
            <MetricCard
              label="Time to 1st Tool"
              metric={metrics.time_to_first_tool_ms}
              format={(v) => `${v.toFixed(0)}ms`}
              thresholds={{ good: 2000, warning: 5000, lowerIsBetter: true }}
            />
          </section>

          {selectedRun && (
            <>
              <section className="bg-surface-2 rounded-xl border border-border p-4">
                <h3 className="text-[13px] font-semibold text-text-1 mb-3">
                  Timeline
                </h3>
                <TimelineView steps={selectedRun.steps} />
              </section>
              <section className="bg-surface-2 rounded-xl border border-border p-4">
                <ToolUsageChart steps={selectedRun.steps} />
              </section>
              <FailureModePanel steps={selectedRun.steps} />
            </>
          )}
        </div>
      )}
    </div>
  );
}
