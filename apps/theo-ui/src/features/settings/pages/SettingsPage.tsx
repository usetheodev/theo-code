import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useOutletContext } from "react-router-dom";
import { clsx } from "clsx";
import { Check, LogOut, ExternalLink, FolderOpen } from "lucide-react";
import { AppConfig, AuthStatus, AppLayoutContext } from "../../../types";

const PRESETS = [
  { label: "OpenAI", url: "https://api.openai.com", model: "gpt-4o" },
  { label: "o3-mini", url: "https://api.openai.com", model: "o3-mini" },
  { label: "Anthropic", url: "https://api.anthropic.com", model: "claude-sonnet-4-20250514" },
  { label: "Local", url: "http://localhost:8000", model: "" },
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

  useEffect(() => { loadState(); }, []);

  async function loadState() {
    try {
      const [cfg, authStatus, dir] = await Promise.all([
        invoke<AppConfig>("get_config"),
        invoke<AuthStatus>("auth_status"),
        invoke<string | null>("get_project_dir"),
      ]);
      setConfig(cfg);
      setAuth(authStatus);
      setBaseUrl(cfg.base_url);
      setModel(cfg.model);
      setProjectDir(dir || "");
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

          {/* Provider */}
          <Section title="Provider">
            <div className="flex gap-2 flex-wrap mb-4">
              {PRESETS.map((p, i) => {
                const isActive = baseUrl === p.url && model === p.model;
                return (
                  <button
                    key={i}
                    onClick={() => { if (p.url) setBaseUrl(p.url); if (p.model) setModel(p.model); }}
                    className={clsx(
                      "px-3 py-1.5 text-[12px] rounded-md border transition-colors",
                      isActive
                        ? "bg-brand/10 border-brand/30 text-brand font-medium"
                        : "bg-surface-2 border-border text-text-2 hover:bg-surface-3",
                    )}
                  >
                    {p.label}
                  </button>
                );
              })}
            </div>
            <div className="flex flex-col gap-3">
              <Field label="Base URL" value={baseUrl} onChange={setBaseUrl} placeholder="https://api.openai.com" />
              <Field label="Model" value={model} onChange={setModel} placeholder="gpt-4o" />
              <Field
                label={auth?.authenticated ? "API Key (optional)" : "API Key"}
                value={apiKey}
                onChange={setApiKey}
                placeholder={config?.has_api_key ? "\u2022\u2022\u2022\u2022\u2022\u2022 (configured)" : "sk-..."}
                type="password"
              />
            </div>
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

function Field({
  label, value, onChange, placeholder, type = "text",
}: {
  label: string; value: string; onChange: (v: string) => void; placeholder: string; type?: string;
}) {
  return (
    <div>
      <label className="block text-[12px] text-text-2 mb-1">{label}</label>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors"
      />
    </div>
  );
}
