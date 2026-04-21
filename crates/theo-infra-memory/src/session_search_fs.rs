//! Filesystem-backed cross-session keyword search (Phase 1 T1.4).
//!
//! Plan: `docs/plans/PLAN_MEMORY_SUPERIORITY.md` §Task 1.4.
//! Trait: `theo_domain::session_search::SessionSearch`.
//!
//! Scans every JSON episode under `.theo/memory/episodes/` and ranks by
//! `keyword_overlap * 0.6 + recency * 0.4`. Designed to be <50ms for
//! ~100 episodes (AC-1.4.6) by streaming `serde_json::from_str`; no
//! external index required. Migrates to `MemoryRetrieval` + RRF when
//! T3.3 unblocks.

use std::path::PathBuf;

use theo_domain::episode::EpisodeSummary;
use theo_domain::session_search::{
    SessionSearch, SessionSearchHit, extract_keywords, rank_episode,
};

/// Filesystem implementation of `SessionSearch`.
pub struct FsSessionSearch {
    project_dir: PathBuf,
}

impl FsSessionSearch {
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        Self {
            project_dir: project_dir.into(),
        }
    }

    fn load_episodes(&self) -> Vec<EpisodeSummary> {
        // Primary + legacy path (matches StateManager::load_episode_summaries).
        let mut out = Vec::new();
        for sub in [
            self.project_dir.join(".theo/memory/episodes"),
            self.project_dir.join(".theo/wiki/episodes"),
        ] {
            if !sub.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&sub) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    if let Ok(raw) = std::fs::read_to_string(&path) {
                        if let Ok(ep) = serde_json::from_str::<EpisodeSummary>(&raw) {
                            out.push(ep);
                        }
                    }
                }
            }
        }
        out
    }
}

impl SessionSearch for FsSessionSearch {
    fn search(&self, query: &str, max_results: usize) -> Vec<SessionSearchHit> {
        let q = extract_keywords(query);
        if q.is_empty() {
            return Vec::new();
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let episodes = self.load_episodes();

        let mut scored: Vec<(f32, Vec<String>, EpisodeSummary)> = episodes
            .into_iter()
            .map(|ep| {
                let (s, m) = rank_episode(&q, &ep, now_ms);
                (s, m, ep)
            })
            // Keep only episodes with at least one keyword match —
            // a 0-keyword episode with strong recency is not relevant.
            .filter(|(_, matched, _)| !matched.is_empty())
            .collect();

        // Sort by score descending — using total_cmp to handle NaN safely.
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(max_results);

        scored
            .into_iter()
            .map(|(score, matched, ep)| SessionSearchHit {
                summary_id: ep.summary_id,
                run_id: ep.run_id,
                objective: ep.machine_summary.objective,
                rank_score: score,
                matched_terms: matched,
            })
            .collect()
    }
}

/// Render a list of hits as a compact text block suitable for LLM
/// injection (AC-1.4.5).
pub fn render_hits(hits: &[SessionSearchHit]) -> String {
    if hits.is_empty() {
        return "Nenhuma sessao anterior relevante.".to_string();
    }
    let mut out = String::from("## Past sessions matching the query\n");
    for h in hits {
        out.push_str(&format!(
            "- `{}` (score {:.2}): {} — matched: {}\n",
            h.run_id,
            h.rank_score,
            h.objective,
            h.matched_terms.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_ep(
        dir: &std::path::Path,
        id: &str,
        objective: &str,
        actions: &[&str],
        created_at: u64,
    ) {
        let ed = dir.join(".theo/memory/episodes");
        std::fs::create_dir_all(&ed).expect("t");
        let p = json!({
            "summary_id": id, "run_id": id,
            "task_id": null,
            "window_start_event_id": "", "window_end_event_id": "",
            "machine_summary": {
                "objective": objective,
                "key_actions": actions,
                "outcome": "Success",
                "successful_steps": [],
                "failed_attempts": [],
                "learned_constraints": [],
                "files_touched": []
            },
            "human_summary": null,
            "evidence_event_ids": [],
            "affected_files": [],
            "open_questions": [],
            "unresolved_hypotheses": [],
            "referenced_community_ids": [],
            "supersedes_summary_id": null,
            "schema_version": 1,
            "created_at": created_at,
            "ttl_policy": "RunScoped",
            "lifecycle": "Active"
        });
        std::fs::write(
            ed.join(format!("{id}.json")),
            serde_json::to_string(&p).expect("t"),
        )
        .expect("t");
    }

    // ── AC-1.4.3: keyword search matches across fields ───────────
    #[test]
    fn test_t1_4_ac_3_keyword_match_in_objective() {
        let dir = tempfile::tempdir().expect("t");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("t")
            .as_millis() as u64;
        write_ep(dir.path(), "ep-1", "fix login bug", &[], now_ms);
        write_ep(dir.path(), "ep-2", "refactor database", &[], now_ms);

        let s = FsSessionSearch::new(dir.path());
        let hits = s.search("login", 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].run_id, "ep-1");
    }

    // ── AC-1.4.4: rank by keyword_overlap * 0.6 + recency * 0.4 ──
    #[test]
    fn test_t1_4_ac_4_rank_combines_keyword_and_recency() {
        let dir = tempfile::tempdir().expect("t");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("t")
            .as_millis() as u64;
        let one_week_ago = now_ms.saturating_sub(7 * 24 * 3600 * 1000);

        // Both match the keyword, but one is old → should rank lower.
        write_ep(dir.path(), "ep-recent", "fix login", &[], now_ms);
        write_ep(dir.path(), "ep-old", "fix login", &[], one_week_ago);

        let s = FsSessionSearch::new(dir.path());
        let hits = s.search("login", 3);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].run_id, "ep-recent");
        assert!(hits[0].rank_score > hits[1].rank_score);
    }

    // ── AC-1.4.5: max 3 results, structured text format ──────────
    #[test]
    fn test_t1_4_ac_5_caps_at_max_results() {
        let dir = tempfile::tempdir().expect("t");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("t")
            .as_millis() as u64;
        for i in 0..10 {
            write_ep(dir.path(), &format!("ep-{i}"), "fix login", &[], now_ms);
        }
        let s = FsSessionSearch::new(dir.path());
        let hits = s.search("login", 3);
        assert_eq!(hits.len(), 3);

        let rendered = render_hits(&hits);
        assert!(rendered.contains("Past sessions"));
        assert_eq!(rendered.matches("score").count(), 3);
    }

    // ── AC-1.4.6: performance <50ms with 100 episodes ────────────
    #[test]
    fn test_t1_4_ac_6_performance_under_50ms_with_100_episodes() {
        let dir = tempfile::tempdir().expect("t");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("t")
            .as_millis() as u64;
        for i in 0..100 {
            write_ep(
                dir.path(),
                &format!("ep-{i:03}"),
                "fix bug in module",
                &["read", "edit"],
                now_ms.saturating_sub(i as u64 * 1000),
            );
        }
        let s = FsSessionSearch::new(dir.path());
        let start = std::time::Instant::now();
        let hits = s.search("bug", 3);
        let elapsed = start.elapsed();
        assert_eq!(hits.len(), 3);
        assert!(
            elapsed.as_millis() < 50,
            "search must complete in <50ms for 100 episodes, took {:?}",
            elapsed
        );
    }

    // ── AC-1.4.7: no results → explanatory placeholder ───────────
    #[test]
    fn test_t1_4_ac_7_no_results_yields_placeholder() {
        let dir = tempfile::tempdir().expect("t");
        let s = FsSessionSearch::new(dir.path());
        let hits = s.search("anything", 3);
        assert!(hits.is_empty());
        let rendered = render_hits(&hits);
        assert!(rendered.contains("Nenhuma sessao"));
    }
}
