import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface WikiPageMeta {
  slug: string;
  namespace: string;
  title: string;
  last_compile_unix: number;
}

export interface LintIssue {
  metric: string;
  severity: "info" | "concern" | "warning" | "critical";
  message: string;
}

export function MemoryWikiPage() {
  const [pages, setPages] = useState<WikiPageMeta[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [body, setBody] = useState<string>("");
  const [lint, setLint] = useState<LintIssue[]>([]);
  const [compileError, setCompileError] = useState<string | null>(null);

  useEffect(() => {
    invoke<WikiPageMeta[]>("list_wiki_pages").then(setPages).catch(() => {});
    invoke<LintIssue[]>("run_wiki_lint").then(setLint).catch(() => {});
  }, []);

  const open = async (slug: string) => {
    setActive(slug);
    const md = await invoke<string>("get_wiki_page", { slug });
    setBody(md);
  };

  const compile = async () => {
    setCompileError(null);
    try {
      await invoke("trigger_wiki_compile");
    } catch (e) {
      setCompileError(String(e));
    }
  };

  return (
    <div className="p-6 grid grid-cols-12 gap-6">
      <aside className="col-span-4 border-r pr-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Pages</h2>
          <button
            className="text-xs px-2 py-1 rounded bg-blue-50 text-blue-600 hover:bg-blue-100"
            onClick={compile}
            aria-label="Trigger wiki compile"
          >
            Compile
          </button>
        </div>
        {compileError && (
          <div className="mb-3 text-xs text-red-600">{compileError}</div>
        )}
        {pages.length === 0 ? (
          <p className="text-sm text-gray-500">No compiled pages.</p>
        ) : (
          <ul className="space-y-1">
            {pages.map((p) => (
              <li key={p.slug}>
                <button
                  className={
                    "w-full text-left px-2 py-1 rounded text-sm " +
                    (active === p.slug
                      ? "bg-blue-100 text-blue-800"
                      : "hover:bg-gray-100")
                  }
                  onClick={() => open(p.slug)}
                >
                  <span className="text-gray-500">[{p.namespace}] </span>
                  {p.title || p.slug}
                </button>
              </li>
            ))}
          </ul>
        )}
        {lint.length > 0 && (
          <div className="mt-6">
            <h3 className="text-sm font-semibold mb-2">Lint</h3>
            <ul className="space-y-1 text-xs">
              {lint.map((i, idx) => (
                <li key={`${i.metric}-${idx}`} className="text-gray-700">
                  <span
                    className={
                      "inline-block w-16 font-mono " +
                      (i.severity === "critical"
                        ? "text-red-600"
                        : i.severity === "warning"
                          ? "text-amber-600"
                          : i.severity === "concern"
                            ? "text-orange-600"
                            : "text-gray-500")
                    }
                  >
                    [{i.severity}]
                  </span>
                  <span className="ml-2">{i.message}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </aside>
      <main className="col-span-8">
        {active ? (
          <article className="prose max-w-none">
            {/* Minimal markdown viewer — full syntax highlight lands
                once the markdown renderer is shared across features. */}
            <pre className="whitespace-pre-wrap font-mono text-sm bg-gray-50 p-4 rounded">
              {body || "(empty)"}
            </pre>
          </article>
        ) : (
          <p className="text-gray-500">Select a page to view its content.</p>
        )}
      </main>
    </div>
  );
}
