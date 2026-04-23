import { ObservabilityDashboard } from "../../observability/pages/ObservabilityDashboard";

/**
 * Legacy /monitoring entry point — delegates to the observability dashboard.
 * Kept as a thin wrapper so external links remain valid.
 */
export function MonitoringPage() {
  return <ObservabilityDashboard />;
}
