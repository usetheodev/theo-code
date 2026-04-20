import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface MemorySettings {
  retention_days: number;
  forgetting_enabled: boolean;
  privacy_commit_gitignore: boolean;
}

export function MemorySettingsPage() {
  const [settings, setSettings] = useState<MemorySettings | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<MemorySettings>("get_memory_settings")
      .then(setSettings)
      .catch((e) => setError(String(e)));
  }, []);

  const save = async () => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("save_memory_settings", { settings });
      setSavedAt(new Date());
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (!settings) return <div className="p-6">Loading settings…</div>;

  return (
    <div className="p-6 max-w-2xl space-y-8">
      <h1 className="text-2xl font-semibold">Memory — Settings</h1>

      <section>
        <h2 className="text-lg font-medium mb-2">Retention</h2>
        <label className="block">
          <span className="text-sm text-gray-700">
            Keep episodes for (days)
          </span>
          <input
            type="number"
            min={1}
            max={365}
            value={settings.retention_days}
            onChange={(e) =>
              setSettings({
                ...settings,
                retention_days: parseInt(e.target.value, 10) || 1,
              })
            }
            className="mt-1 block w-32 rounded border border-gray-300 px-2 py-1"
          />
        </label>
      </section>

      <section>
        <h2 className="text-lg font-medium mb-2">Forgetting</h2>
        <label className="inline-flex items-center gap-2">
          <input
            type="checkbox"
            checked={settings.forgetting_enabled}
            onChange={(e) =>
              setSettings({ ...settings, forgetting_enabled: e.target.checked })
            }
          />
          <span className="text-sm text-gray-700">
            Enable periodic forgetting of low-signal lessons
          </span>
        </label>
      </section>

      <section>
        <h2 className="text-lg font-medium mb-2">Privacy</h2>
        <label className="inline-flex items-center gap-2">
          <input
            type="checkbox"
            checked={settings.privacy_commit_gitignore}
            onChange={(e) =>
              setSettings({
                ...settings,
                privacy_commit_gitignore: e.target.checked,
              })
            }
          />
          <span className="text-sm text-gray-700">
            Ensure .theo/memory/ stays in .gitignore
          </span>
        </label>
      </section>

      <div className="flex items-center gap-3 pt-4 border-t">
        <button
          onClick={save}
          disabled={saving}
          className="px-4 py-2 rounded bg-blue-600 text-white disabled:bg-gray-400"
        >
          {saving ? "Saving…" : "Save"}
        </button>
        {savedAt && (
          <span className="text-sm text-green-600">
            Saved at {savedAt.toLocaleTimeString()}
          </span>
        )}
        {error && <span className="text-sm text-red-600">{error}</span>}
      </div>
    </div>
  );
}
