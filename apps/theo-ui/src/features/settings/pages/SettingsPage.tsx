import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useOutletContext } from "react-router-dom";
import { clsx } from "clsx";
import { Check, LogOut, ExternalLink, FolderOpen, GitBranch } from "lucide-react";
import { AppConfig, AuthStatus, AppLayoutContext } from "../../../types";
import { DeviceAuthDialog } from "../../../components/auth/DeviceAuthDialog";
// Anthropic uses API key — no OAuth dialog needed

type ProviderId = "openai" | "anthropic" | "copilot" | "local" | "custom";

const PRESETS: { id: ProviderId; label: string; url: string }[] = [
  { id: "openai", label: "OpenAI", url: "https://api.openai.com" },
  { id: "anthropic", label: "Anthropic", url: "https://api.anthropic.com" },
  { id: "copilot", label: "Copilot", url: "https://api.githubcopilot.com" },
  { id: "local", label: "Local", url: "http://localhost:8000" },
];

export function SettingsPage() {
  const { reloadState } = useOutletContext<AppLayoutContext>();
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
  const [copilotStatus, setCopilotStatus] = useState<{ authenticated: boolean; domain?: string } | null>(null);
  // Anthropic status kept for preset badge (API key auth, not OAuth)
  const [anthropicStatus, setAnthropicStatus] = useState<{ authenticated: boolean; email?: string } | null>(null);

  useEffect(() => { loadState(); }, []);

  // Fetch models when provider changes
  useEffect(() => {
    if (activeProvider === "local" || activeProvider === "custom") {
      setProviderModels([]);
      return;
    }
    let cancelled = false;
    setLoadingModels(true);
    invoke<{ models: string[]; default: string }>("provider_models", { provider: activeProvider })
      .then((result) => {
        if (cancelled) return;
        setProviderModels(result.models);
        setDefaultModel(result.default);
        // If current model is not in the list, set the default
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
    return () => { cancelled = true; };
  }, [activeProvider]);

  async function loadState() {
    try {
      const [cfg, authStatus, dir, copilot, anthro] = await Promise.all([
        invoke<AppConfig>("get_config"),
        invoke<AuthStatus>("auth_status"),
        invoke<string | null>("get_project_dir"),
        invoke<{ authenticated: boolean; domain?: string }>("copilot_status").catch(() => null),
        invoke<{ authenticated: boolean; email?: string }>("anthropic_status").catch(() => null),
      ]);
      setConfig(cfg);
      setAuth(authStatus);
      setBaseUrl(cfg.base_url);
      setModel(cfg.model);
      setProjectDir(dir || "");
      if (copilot) setCopilotStatus(copilot);
      if (anthro) setAnthropicStatus(anthro);
      // Detect active provider from URL
      const matched = PRESETS.find((p) => cfg.base_url.includes(new URL(p.url).hostname));
      if (matched) setActiveProvider(matched.id);
    } catch (e) {
      console.error("Failed to load config:", e);
    }
  }

  async function handleSave() {
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
  }

  async function handleOAuthLogin() {
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
  }

  async function handleLogout() {
    try {
      await invoke("auth_logout");
      await loadState();
      await reloadState();
    } catch (e) {
      alert(`Logout failed: ${e}`);
    }
  }

  return (
    <div className="flex-1 flex flex-col">
      <div className="flex items-center px-6 h-[52px] border-b border-border shrink-0">
        <h2 className="text-[15px] font-semibold text-text-0">Settings</h2>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="max-w-lg mx-auto px-6 py-8 flex flex-col gap-8">

          {/* Auth */}
          <Section title="Authentication">
            {auth?.authenticated ? (
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-ok" />
                  <span className="text-[13px] text-text-1">
                    Connected
                    {auth.account_id && <span className="text-text-3 ml-1">({auth.account_id})</span>}
                  </span>
                </div>
                <button
                  onClick={handleLogout}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-[12px] text-text-2 hover:text-err border border-border rounded-md hover:border-err/30 transition-colors"
                >
                  <LogOut size={12} />
                  Logout
                </button>
              </div>
            ) : (
              <div>
                <button
                  onClick={handleOAuthLogin}
                  disabled={authLoading}
                  className="flex items-center gap-2 px-4 py-2 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors disabled:opacity-50"
                >
                  <ExternalLink size={14} />
                  {authLoading ? "Opening browser..." : "Login with OpenAI"}
                </button>
                <p className="text-[11px] text-text-3 mt-2">
                  Or enter an API key below
                </p>
              </div>
            )}
          </Section>

          {/* Copilot Auth */}
          <Section title="GitHub Copilot">
            {copilotStatus?.authenticated ? (
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-ok" />
                  <span className="text-[13px] text-text-1">
                    Connected
                    {copilotStatus.domain && copilotStatus.domain !== "github.com" && (
                      <span className="text-text-3 ml-1">({copilotStatus.domain})</span>
                    )}
                  </span>
                </div>
                <button
                  onClick={async () => {
                    try {
                      await invoke("copilot_logout");
                      await loadState();
                      await reloadState();
                    } catch (e) {
                      alert(`Logout failed: ${e}`);
                    }
                  }}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-[12px] text-text-2 hover:text-err border border-border rounded-md hover:border-err/30 transition-colors"
                >
                  <LogOut size={12} />
                  Disconnect
                </button>
              </div>
            ) : (
              <div>
                <button
                  onClick={() => setCopilotDialogOpen(true)}
                  className="flex items-center gap-2 px-4 py-2 bg-surface-3 border border-border rounded-lg text-[13px] text-text-0 hover:bg-surface-4 transition-colors"
                >
                  <GitBranch size={14} />
                  Login with GitHub Copilot
                </button>
                <p className="text-[11px] text-text-3 mt-2">
                  Use your GitHub Copilot subscription
                </p>
              </div>
            )}
            <DeviceAuthDialog
              open={copilotDialogOpen}
              onOpenChange={setCopilotDialogOpen}
              onSuccess={async () => {
                await loadState();
                await reloadState();
                setActiveProvider("copilot");
                setBaseUrl("https://api.githubcopilot.com");
                // Model will be set by the useEffect that fetches provider_models
              }}
            />
          </Section>

          {/* Provider */}
          <Section title="Provider">
            <div className="flex gap-2 flex-wrap mb-4">
              {PRESETS.map((p) => {
                const isCopilot = p.id === "copilot";
                const isAnthropic = p.id === "anthropic";
                const isActive = activeProvider === p.id;
                const needsAuth = isCopilot && !copilotStatus?.authenticated;
                return (
                  <button
                    key={p.id}
                    onClick={async () => {
                      if (isCopilot && !copilotStatus?.authenticated) {
                        setCopilotDialogOpen(true);
                        return;
                      }
                      setActiveProvider(p.id);
                      setBaseUrl(p.url);
                      if (isCopilot && copilotStatus?.authenticated) {
                        await invoke("copilot_apply_to_config", { model: null }).catch(() => {});
                        await loadState();
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
                    {(isCopilot && copilotStatus?.authenticated) && <span className="w-1.5 h-1.5 rounded-full bg-ok" />}
                    {(isAnthropic && anthropicStatus?.authenticated) && <span className="w-1.5 h-1.5 rounded-full bg-ok" />}
                    {needsAuth && <span className="text-[10px] text-text-3">login</span>}
                  </button>
                );
              })}
            </div>
            {(() => {
              const isCopilotActive = activeProvider === "copilot";
              const hasModelList = providerModels.length > 0;
              return (
                <div className="flex flex-col gap-3">
                  <Field label="Base URL" value={baseUrl} onChange={setBaseUrl} placeholder="https://api.openai.com" />
                  {hasModelList ? (
                    <ModelSelect
                      models={providerModels}
                      value={model}
                      onChange={setModel}
                      loading={loadingModels}
                    />
                  ) : (
                    <Field
                      label="Model"
                      value={model}
                      onChange={setModel}
                      placeholder="model name"
                    />
                  )}
                  <Field
                    label={isCopilotActive ? "API Key (managed by Copilot)" : auth?.authenticated ? "API Key (optional)" : "API Key"}
                    value={apiKey}
                    onChange={setApiKey}
                    placeholder={isCopilotActive ? "OAuth token managed automatically" : config?.has_api_key ? "\u2022\u2022\u2022\u2022\u2022\u2022 (configured)" : "sk-..."}
                    type="password"
                    disabled={isCopilotActive && copilotStatus?.authenticated}
                  />
                </div>
              );
            })()}
          </Section>

          {/* Project */}
          <Section title="Project">
            <div className="flex gap-2">
              <input
                value={projectDir}
                onChange={(e) => setProjectDir(e.target.value)}
                placeholder="/path/to/project"
                className="flex-1 px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors"
              />
              <button
                onClick={async () => {
                  const selected = await open({ directory: true, multiple: false, title: "Select Project" });
                  if (selected) setProjectDir(selected as string);
                }}
                className="flex items-center gap-1.5 px-3 py-2 bg-surface-2 border border-border rounded-lg text-[12px] text-text-2 hover:bg-surface-3 transition-colors shrink-0"
              >
                <FolderOpen size={14} />
                Browse
              </button>
            </div>
          </Section>

          {/* Save */}
          <div className="flex justify-end pt-2">
            <button
              onClick={handleSave}
              disabled={saving}
              className={clsx(
                "flex items-center gap-2 px-5 py-2 rounded-lg text-[13px] font-medium transition-all",
                saved
                  ? "bg-ok/10 text-ok border border-ok/20"
                  : "bg-brand text-white hover:bg-brand-hover",
                saving && "opacity-50",
              )}
            >
              {saved && <Check size={14} />}
              {saved ? "Saved" : saving ? "Saving..." : "Save settings"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="text-[12px] font-semibold text-text-3 uppercase tracking-widest mb-3">
        {title}
      </h3>
      {children}
    </div>
  );
}

function ModelSelect({
  models,
  value,
  onChange,
  loading = false,
}: {
  models: string[];
  value: string;
  onChange: (v: string) => void;
  loading?: boolean;
}) {
  const [customMode, setCustomMode] = useState(false);

  if (customMode) {
    return (
      <div>
        <label className="block text-[12px] text-text-2 mb-1">
          Model
          <button
            onClick={() => setCustomMode(false)}
            className="ml-2 text-brand hover:text-brand-hover transition-colors"
          >
            back to list
          </button>
        </label>
        <input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="custom model name"
          className="w-full px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors"
        />
      </div>
    );
  }

  if (loading) {
    return (
      <div>
        <label className="block text-[12px] text-text-2 mb-1">Model</label>
        <div className="w-full px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-3">
          Loading models...
        </div>
      </div>
    );
  }

  return (
    <div>
      <label className="block text-[12px] text-text-2 mb-1">Model</label>
      <select
        value={models.includes(value) ? value : ""}
        onChange={(e) => {
          if (e.target.value === "__custom__") {
            setCustomMode(true);
            return;
          }
          onChange(e.target.value);
        }}
        className="w-full px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 outline-none focus:border-border-focus transition-colors appearance-none cursor-pointer"
      >
        {!models.includes(value) && value && (
          <option value="" disabled>
            {value} (not in list)
          </option>
        )}
        {models.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
        <option value="__custom__">Custom...</option>
      </select>
    </div>
  );
}

function Field({
  label, value, onChange, placeholder, type = "text", disabled = false,
}: {
  label: string; value: string; onChange: (v: string) => void; placeholder: string; type?: string; disabled?: boolean;
}) {
  return (
    <div>
      <label className="block text-[12px] text-text-2 mb-1">{label}</label>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        className={clsx(
          "w-full px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors",
          disabled && "opacity-50 cursor-not-allowed",
        )}
      />
    </div>
  );
}
