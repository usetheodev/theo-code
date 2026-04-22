import { createHashRouter, Navigate } from "react-router-dom";
import { AppLayout } from "./AppLayout";
import { AssistantPage } from "../features/assistant/pages/AssistantPage";
import { LogsPage } from "../features/logs/pages/LogsPage";
import { CodePage } from "../features/code/pages/CodePage";
import { DeploysPage } from "../features/deploys/pages/DeploysPage";
import { MonitoringPage } from "../features/monitoring/pages/MonitoringPage";
import { ObservabilityDashboard } from "../features/observability/pages/ObservabilityDashboard";
import { DatabasePage } from "../features/database/pages/DatabasePage";
import { SettingsPage } from "../features/settings/pages/SettingsPage";
import { EpisodesPage } from "../features/memory/pages/EpisodesPage";
import { MemoryWikiPage } from "../features/memory/pages/WikiPage";
import { MemorySettingsPage } from "../features/memory/pages/SettingsPage";

export const router = createHashRouter([
  {
    path: "/",
    element: <AppLayout />,
    children: [
      { index: true, element: <Navigate to="/assistant" replace /> },
      { path: "assistant", element: <AssistantPage /> },
      { path: "logs", element: <LogsPage /> },
      { path: "code", element: <CodePage /> },
      { path: "deploys", element: <DeploysPage /> },
      { path: "monitoring", element: <MonitoringPage /> },
      { path: "observability", element: <ObservabilityDashboard /> },
      { path: "database", element: <DatabasePage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "memory/episodes", element: <EpisodesPage /> },
      { path: "memory/wiki", element: <MemoryWikiPage /> },
      { path: "memory/settings", element: <MemorySettingsPage /> },
    ],
  },
]);
