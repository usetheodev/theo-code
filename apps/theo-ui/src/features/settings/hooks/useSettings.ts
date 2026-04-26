// T4.4 — Settings state + actions extracted into a custom hook.
//
// Before this file the SettingsPage component held ~135 LOC of state
// declarations, effects, and async handlers. Pulling them into a hook:
// (a) shrinks the page component to its JSX duties,
// (b) makes every action unit-testable via React-Testing-Library,
// (c) lets future sub-components consume the same state without prop drilling.

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AppConfig, AuthStatus } from "../../../types";

export type ProviderId = "openai" | "anthropic" | "copilot" | "local" | "custom";

export interface ProviderPreset {
  id: ProviderId;
  label: string;
  url: string;
}

export const PROVIDER_PRESETS: ProviderPreset[] = [
  { id: "openai", label: "OpenAI", url: "https://api.openai.com" },
  { id: "anthropic", label: "Anthropic", url: "https://api.anthropic.com" },
  { id: "copilot", label: "Copilot", url: "https://api.githubcopilot.com" },
  { id: "local", label: "Local", url: "http://localhost:8000" },
];

export interface CopilotStatus {
  authenticated: boolean;
  domain?: string;
}

export interface AnthropicStatus {
  authenticated: boolean;
  email?: string;
}

export interface UseSettingsArgs {
  /** Called after any mutation to refresh outer layout state. */
  reloadState: () => Promise<void> | void;
}

export function useSettings({ reloadState }: UseSettingsArgs) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [projectDir, setProjectDir] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [authLoading, setAuthLoading] = useState(false);
  const [activeProvider, setActiveProvider] = useState<ProviderId>("openai");
  const [providerModels, setProviderModels] = useState<string[]>([]);
  const [defaultModel, setDefaultModel] = useState("");
  const [loadingModels, setLoadingModels] = useState(false);
  const [copilotDialogOpen, setCopilotDialogOpen] = useState(false);
  const [copilotStatus, setCopilotStatus] = useState<CopilotStatus | null>(null);
  const [anthropicStatus, setAnthropicStatus] = useState<AnthropicStatus | null>(null);

  const loadState = useCallback(async () => {
    try {
      const [cfg, authStatus, dir, copilot, anthro] = await Promise.all([
        invoke<AppConfig>("get_config"),
        invoke<AuthStatus>("auth_status"),
        invoke<string | null>("get_project_dir"),
        invoke<CopilotStatus>("copilot_status").catch(() => null),
        invoke<AnthropicStatus>("anthropic_status").catch(() => null),
      ]);
      setConfig(cfg);
      setAuth(authStatus);
      setBaseUrl(cfg.base_url);
      setModel(cfg.model);
      setProjectDir(dir || "");
      if (copilot) setCopilotStatus(copilot);
      if (anthro) setAnthropicStatus(anthro);
      const matched = PROVIDER_PRESETS.find((p) =>
        cfg.base_url.includes(new URL(p.url).hostname),
      );
      if (matched) setActiveProvider(matched.id);
    } catch (e) {
      console.error("Failed to load config:", e);
    }
  }, []);

  useEffect(() => {
    loadState();
  }, [loadState]);

  // Fetch models when provider changes.
  useEffect(() => {
    if (activeProvider === "local" || activeProvider === "custom") {
      setProviderModels([]);
      return;
    }
    let cancelled = false;
    setLoadingModels(true);
    invoke<{ models: string[]; default: string }>("provider_models", {
      provider: activeProvider,
    })
      .then((result) => {
        if (cancelled) return;
        setProviderModels(result.models);
        setDefaultModel(result.default);
        // If current model is not in the list, adopt the default.
        if (result.models.length > 0 && !result.models.includes(model)) {
          setModel(result.default);
        }
      })
      .catch(() => {
        if (cancelled) return;
        setProviderModels([]);
      })
      .finally(() => {
        if (!cancelled) setLoadingModels(false);
      });
    return () => {
      cancelled = true;
    };
    // `model` intentionally excluded from deps — we only refetch on provider change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeProvider]);

  const save = useCallback(async () => {
    setSaving(true);
    setSaved(false);
    try {
      await invoke("update_config", {
        baseUrl: baseUrl || undefined,
        model: model || undefined,
        apiKey: apiKey || undefined,
      });
      if (projectDir) {
        await invoke("set_project_dir", { path: projectDir });
      }
      setSaved(true);
      await reloadState();
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      alert(`Error: ${e}`);
    } finally {
      setSaving(false);
    }
  }, [apiKey, baseUrl, model, projectDir, reloadState]);

  const loginOAuth = useCallback(async () => {
    setAuthLoading(true);
    try {
      await invoke("auth_login_browser");
      await invoke<boolean>("auth_apply_to_config");
      await loadState();
      await reloadState();
    } catch (e) {
      alert(`OAuth failed: ${e}`);
    } finally {
      setAuthLoading(false);
    }
  }, [loadState, reloadState]);

  const logoutOAuth = useCallback(async () => {
    try {
      await invoke("auth_logout");
      await loadState();
      await reloadState();
    } catch (e) {
      alert(`Logout failed: ${e}`);
    }
  }, [loadState, reloadState]);

  const logoutCopilot = useCallback(async () => {
    try {
      await invoke("copilot_logout");
      await loadState();
      await reloadState();
    } catch (e) {
      alert(`Logout failed: ${e}`);
    }
  }, [loadState, reloadState]);

  const onCopilotSuccess = useCallback(async () => {
    await loadState();
    await reloadState();
    setActiveProvider("copilot");
    setBaseUrl("https://api.githubcopilot.com");
  }, [loadState, reloadState]);

  return {
    // state
    config,
    auth,
    baseUrl,
    model,
    apiKey,
    projectDir,
    saving,
    saved,
    authLoading,
    activeProvider,
    providerModels,
    defaultModel,
    loadingModels,
    copilotDialogOpen,
    copilotStatus,
    anthropicStatus,
    // setters
    setBaseUrl,
    setModel,
    setApiKey,
    setProjectDir,
    setActiveProvider,
    setCopilotDialogOpen,
    // actions
    loadState,
    save,
    loginOAuth,
    logoutOAuth,
    logoutCopilot,
    onCopilotSuccess,
  } as const;
}
