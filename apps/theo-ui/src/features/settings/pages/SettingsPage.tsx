// T4.4 — SettingsPage, re-composed from `useSettings` + per-section
// components. Prior to this refactor the page held ~135 LOC of state
// plus a 250-LOC render tree in a single function. Extraction moves:
//   - state + handlers → `../hooks/useSettings`
//   - JSX sections    → `./sections/*`
// leaving the page file as a thin composition layer.
//
// Sub-component files intentionally live beside this one so the folder
// stays self-contained:
//   pages/
//     SettingsPage.tsx              (this file — orchestrator)
//     sections/
//       AuthSection.tsx
//       CopilotSection.tsx
//       ProviderSection.tsx
//       ProjectSection.tsx
//       SaveButton.tsx
//       Section.tsx / Field.tsx / ModelSelect.tsx
//   hooks/
//     useSettings.ts                (state + actions)

import { useOutletContext } from "react-router-dom";
import { AppLayoutContext } from "../../../types";
import { useSettings } from "../hooks/useSettings";
import { AuthSection } from "./sections/AuthSection";
import { CopilotSection } from "./sections/CopilotSection";
import { ProviderSection } from "./sections/ProviderSection";
import { ProjectSection } from "./sections/ProjectSection";
import { SaveButton } from "./sections/SaveButton";

export function SettingsPage() {
  const { reloadState } = useOutletContext<AppLayoutContext>();
  const s = useSettings({ reloadState });

  return (
    <div className="flex-1 flex flex-col">
      <div className="flex items-center px-6 h-[52px] border-b border-border shrink-0">
        <h2 className="text-[15px] font-semibold text-text-0">Settings</h2>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="max-w-lg mx-auto px-6 py-8 flex flex-col gap-8">
          <AuthSection
            auth={s.auth}
            authLoading={s.authLoading}
            onLogin={s.loginOAuth}
            onLogout={s.logoutOAuth}
          />
          <CopilotSection
            status={s.copilotStatus}
            dialogOpen={s.copilotDialogOpen}
            onDialogOpenChange={s.setCopilotDialogOpen}
            onLogout={s.logoutCopilot}
            onSuccess={s.onCopilotSuccess}
          />
          <ProviderSection
            activeProvider={s.activeProvider}
            setActiveProvider={s.setActiveProvider}
            baseUrl={s.baseUrl}
            setBaseUrl={s.setBaseUrl}
            model={s.model}
            setModel={s.setModel}
            apiKey={s.apiKey}
            setApiKey={s.setApiKey}
            providerModels={s.providerModels}
            loadingModels={s.loadingModels}
            config={s.config}
            auth={s.auth}
            copilotStatus={s.copilotStatus}
            anthropicStatus={s.anthropicStatus}
            onOpenCopilotDialog={() => s.setCopilotDialogOpen(true)}
            reloadAfterApply={s.loadState}
          />
          <ProjectSection
            projectDir={s.projectDir}
            setProjectDir={s.setProjectDir}
          />
          <SaveButton saving={s.saving} saved={s.saved} onSave={s.save} />
        </div>
      </div>
    </div>
  );
}
