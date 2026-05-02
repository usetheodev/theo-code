import { clsx } from "clsx";
import { invoke } from "@tauri-apps/api/core";
import { AppConfig, AuthStatus } from "../../../../types";
import {
  AnthropicStatus,
  CopilotStatus,
  PROVIDER_PRESETS,
  ProviderId,
} from "../../hooks/useSettings";
import { Field } from "./Field";
import { ModelSelect } from "./ModelSelect";
import { Section } from "./Section";

export interface ProviderSectionProps {
  activeProvider: ProviderId;
  setActiveProvider: (p: ProviderId) => void;
  baseUrl: string;
  setBaseUrl: (v: string) => void;
  model: string;
  setModel: (v: string) => void;
  apiKey: string;
  setApiKey: (v: string) => void;
  providerModels: string[];
  loadingModels: boolean;
  config: AppConfig | null;
  auth: AuthStatus | null;
  copilotStatus: CopilotStatus | null;
  anthropicStatus: AnthropicStatus | null;
  onOpenCopilotDialog: () => void;
  reloadAfterApply: () => Promise<void>;
}

export function ProviderSection(props: ProviderSectionProps) {
  const {
    activeProvider,
    setActiveProvider,
    baseUrl,
    setBaseUrl,
    model,
    setModel,
    apiKey,
    setApiKey,
    providerModels,
    loadingModels,
    config,
    auth,
    copilotStatus,
    anthropicStatus,
    onOpenCopilotDialog,
    reloadAfterApply,
  } = props;

  const isCopilotActive = activeProvider === "copilot";
  const hasModelList = providerModels.length > 0;

  return (
    <Section title="Provider">
      <div className="flex gap-2 flex-wrap mb-4">
        {PROVIDER_PRESETS.map((p) => {
          const isCopilot = p.id === "copilot";
          const isAnthropic = p.id === "anthropic";
          const isActive = activeProvider === p.id;
          const needsAuth = isCopilot && !copilotStatus?.authenticated;
          return (
            <button
              key={p.id}
              onClick={async () => {
                if (isCopilot && !copilotStatus?.authenticated) {
                  onOpenCopilotDialog();
                  return;
                }
                setActiveProvider(p.id);
                setBaseUrl(p.url);
                if (isCopilot && copilotStatus?.authenticated) {
                  await invoke("copilot_apply_to_config", { model: null }).catch(
                    () => {},
                  );
                  await reloadAfterApply();
                }
              }}
              className={clsx(
                "px-3 py-1.5 text-[12px] rounded-md border transition-colors inline-flex items-center gap-1.5",
                isActive
                  ? "bg-brand/10 border-brand/30 text-brand font-medium"
                  : "bg-surface-2 border-border text-text-2 hover:bg-surface-3",
              )}
            >
              {p.label}
              {isCopilot && copilotStatus?.authenticated && (
                <span className="w-1.5 h-1.5 rounded-full bg-ok" />
              )}
              {isAnthropic && anthropicStatus?.authenticated && (
                <span className="w-1.5 h-1.5 rounded-full bg-ok" />
              )}
              {needsAuth && <span className="text-[10px] text-text-3">login</span>}
            </button>
          );
        })}
      </div>
      <div className="flex flex-col gap-3">
        <Field
          label="Base URL"
          value={baseUrl}
          onChange={setBaseUrl}
          placeholder="https://api.openai.com"
        />
        {hasModelList ? (
          <ModelSelect
            models={providerModels}
            value={model}
            onChange={setModel}
            loading={loadingModels}
          />
        ) : (
          <Field label="Model" value={model} onChange={setModel} placeholder="model name" />
        )}
        <Field
          label={
            isCopilotActive
              ? "API Key (managed by Copilot)"
              : auth?.authenticated
                ? "API Key (optional)"
                : "API Key"
          }
          value={apiKey}
          onChange={setApiKey}
          placeholder={
            isCopilotActive
              ? "OAuth token managed automatically"
              : config?.has_api_key
                ? "•••••• (configured)"
                : "sk-..."
          }
          type="password"
          disabled={isCopilotActive && copilotStatus?.authenticated}
        />
      </div>
    </Section>
  );
}
