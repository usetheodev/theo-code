import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface EpisodeSummary {
  id: string;
  occurred_at_unix: number;
  title: string;
  summary: string;
}

export function EpisodesPage() {
  const [episodes, setEpisodes] = useState<EpisodeSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<EpisodeSummary[]>("get_episodes", { limit: 50, offset: 0 })
      .then((rows) => {
        setEpisodes(rows);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, []);

  const onDismiss = async (id: string) => {
    try {
      await invoke("dismiss_episode", { id });
      setEpisodes((prev) => prev.filter((e) => e.id !== id));
    } catch (e) {
      setError(String(e));
    }
  };

  if (loading) return <div className="p-6">Loading episodes…</div>;
  if (error) return <div className="p-6 text-red-500">Error: {error}</div>;

  return (
    <div className="p-6 space-y-4">
      <h1 className="text-2xl font-semibold">Memory — Episodes</h1>
      {episodes.length === 0 ? (
        <p className="text-gray-500">No episodes recorded yet.</p>
      ) : (
        <ul className="space-y-3">
          {episodes
            .slice()
            .sort((a, b) => b.occurred_at_unix - a.occurred_at_unix)
            .map((ep) => (
              <li
                key={ep.id}
                className="rounded-lg border border-gray-200 p-4 hover:bg-gray-50"
              >
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <h3 className="font-medium">{ep.title}</h3>
                    <p className="text-sm text-gray-600 mt-1">{ep.summary}</p>
                    <p className="text-xs text-gray-400 mt-2">
                      {new Date(ep.occurred_at_unix * 1000).toISOString()}
                    </p>
                  </div>
                  <button
                    className="text-xs px-2 py-1 rounded bg-red-50 text-red-600 hover:bg-red-100"
                    onClick={() => onDismiss(ep.id)}
                    aria-label={`Dismiss episode ${ep.id}`}
                  >
                    Dismiss
                  </button>
                </div>
              </li>
            ))}
        </ul>
      )}
    </div>
  );
}
