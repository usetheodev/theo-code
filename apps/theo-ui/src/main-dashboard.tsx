import React from "react";
import ReactDOM from "react-dom/client";
import { ObservabilityDashboard } from "./features/observability/pages/ObservabilityDashboard";
import "./styles.css";

// Dedicated browser entry — does not import any Tauri-only module
// (no routes.tsx, no chat/memory/auth pages). Renders the dashboard alone
// so the app can be served as a static bundle over HTTP by
// `theo dashboard`.
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <div className="h-screen bg-surface-0 text-text-1 flex">
      <ObservabilityDashboard />
    </div>
  </React.StrictMode>,
);
