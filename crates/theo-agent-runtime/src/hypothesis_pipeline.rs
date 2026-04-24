//! Hypothesis persistence + feedback loop (Phase 2 T2.3, G6).
//!
//! Plan: `docs/plans/PLAN_MEMORY_SUPERIORITY.md` §Task 2.3.
//! Absorbed reference: CodeTracer (arxiv:2604.11641).
//!
//! After a run, persist any `unresolved_hypotheses` names from the
//! episode to `.theo/memory/hypotheses/{id}.json`. At load time,
//! Active hypotheses are re-injected into the next run's prefetch
//! context (caller's responsibility). Auto-prunes the file when
//! `evidence_against > evidence_for * 2` and total evidence >= 3
//! (delegated to `Hypothesis::should_auto_prune()` in theo-domain).

use std::path::Path;

use theo_domain::episode::{EpisodeSummary, Hypothesis, HypothesisStatus};

/// Serialized envelope with `schema_version` for forward compatibility
/// (decision: meeting 20260420-221947 #11).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct HypothesisEnvelope {
    schema_version: u32,
    hypothesis: Hypothesis,
}

const HYPOTHESIS_SCHEMA_VERSION: u32 = 1;

/// Persist unresolved hypotheses from an episode to
/// `.theo/memory/hypotheses/{id}.json`. Returns the count persisted.
pub fn persist_unresolved(project_dir: &Path, summary: &EpisodeSummary) -> usize {
    if summary.unresolved_hypotheses.is_empty() {
        return 0;
    }
    let dir = project_dir.join(".theo/memory/hypotheses");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        crate::fs_errors::warn_fs_error("hypothesis_pipeline/mkdir", &dir, &e);
    }
    let mut written = 0usize;
    for desc in &summary.unresolved_hypotheses {
        // The domain type `unresolved_hypotheses: Vec<String>` stores
        // descriptions only — wrap each as a fresh Active hypothesis.
        let h = Hypothesis::new(
            &format!("hyp-{}", hash_id(desc, &summary.run_id)),
            desc,
            "from episode unresolved_hypotheses",
        );
        let env = HypothesisEnvelope {
            schema_version: HYPOTHESIS_SCHEMA_VERSION,
            hypothesis: h.clone(),
        };
        let path = dir.join(format!("{}.json", h.id));
        if let Ok(json) = serde_json::to_string_pretty(&env)
            && std::fs::write(&path, json).is_ok()
        {
            written += 1;
        }
    }
    written
}

/// Load Active hypotheses for injection into the next session's context.
/// Auto-prunes (deletes) files whose stored hypothesis is Superseded or
/// `should_auto_prune()` returns true.
pub fn load_active(project_dir: &Path) -> Vec<Hypothesis> {
    let dir = project_dir.join(".theo/memory/hypotheses");
    if !dir.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let parsed = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<HypothesisEnvelope>(&raw).ok());
            let Some(env) = parsed else { continue };

            if env.hypothesis.status == HypothesisStatus::Superseded
                || env.hypothesis.should_auto_prune()
            {
                // Auto-prune: delete the file. Best-effort; log on failure.
                if let Err(e) = std::fs::remove_file(&path) {
                    crate::fs_errors::warn_fs_error(
                        "hypothesis_pipeline/prune",
                        &path,
                        &e,
                    );
                }
                continue;
            }
            if env.hypothesis.status == HypothesisStatus::Active {
                out.push(env.hypothesis);
            }
        }
    }
    out
}

/// Render active hypotheses as a system message block for prefetch
/// injection. Returns empty string when none are active.
pub fn render_active(active: &[Hypothesis]) -> String {
    if active.is_empty() {
        return String::new();
    }
    let mut body = String::from("## Working hypotheses (from prior sessions)\n");
    for h in active {
        body.push_str(&format!(
            "- `{}` (confidence {:.2}): {} — rationale: {}\n",
            h.id, h.confidence, h.description, h.rationale
        ));
    }
    body
}

fn hash_id(desc: &str, run_id: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    desc.hash(&mut h);
    run_id.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_with_hyps(run_id: &str, hyps: &[&str]) -> EpisodeSummary {
        let mut s = EpisodeSummary::from_events(run_id, None, "test", &[]);
        s.unresolved_hypotheses = hyps.iter().map(|s| s.to_string()).collect();
        s
    }

    #[test]
    fn test_t2_3_ac_1_persists_unresolved_hypotheses() {
        let dir = tempfile::tempdir().expect("t");
        let s = summary_with_hyps("r-1", &["bug in jwt.rs", "race condition in cache"]);
        let written = persist_unresolved(dir.path(), &s);
        assert_eq!(written, 2);

        let loaded = load_active(dir.path());
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn test_t2_3_ac_2_stored_with_schema_version() {
        let dir = tempfile::tempdir().expect("t");
        let s = summary_with_hyps("r-1", &["x"]);
        persist_unresolved(dir.path(), &s);

        let hyp_dir = dir.path().join(".theo/memory/hypotheses");
        let files: Vec<_> = std::fs::read_dir(&hyp_dir)
            .expect("t")
            .flatten()
            .collect();
        assert_eq!(files.len(), 1);
        let raw = std::fs::read_to_string(files[0].path()).expect("t");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("t");
        assert_eq!(v.get("schema_version").and_then(|x| x.as_u64()), Some(1));
    }

    #[test]
    fn test_t2_3_ac_4_auto_prune_on_heavy_contradiction() {
        let dir = tempfile::tempdir().expect("t");
        let hyp_dir = dir.path().join(".theo/memory/hypotheses");
        std::fs::create_dir_all(&hyp_dir).expect("t");

        // Write a hypothesis already in auto-prune state: 0 for, 3 against.
        let mut h = Hypothesis::new("hyp-prune", "d", "r");
        for ev in ["e1", "e2", "e3"] {
            h.record_contradiction(ev);
        }
        let env = HypothesisEnvelope {
            schema_version: 1,
            hypothesis: h,
        };
        let path = hyp_dir.join("hyp-prune.json");
        std::fs::write(&path, serde_json::to_string_pretty(&env).expect("t")).expect("t");
        assert!(path.exists());

        let loaded = load_active(dir.path());
        assert!(loaded.is_empty());
        assert!(
            !path.exists(),
            "auto-pruned hypothesis must be deleted from disk"
        );
    }

    #[test]
    fn test_t2_3_render_active_produces_context_block() {
        let h = Hypothesis::new("h1", "desc here", "rationale");
        let out = render_active(&[h]);
        assert!(out.contains("Working hypotheses"));
        assert!(out.contains("desc here"));
    }

    #[test]
    fn test_t2_3_render_active_empty_is_empty() {
        assert_eq!(render_active(&[]), "");
    }

    #[test]
    fn test_t2_3_empty_summary_persists_nothing() {
        let dir = tempfile::tempdir().expect("t");
        let s = EpisodeSummary::from_events("r", None, "none", &[]);
        assert_eq!(persist_unresolved(dir.path(), &s), 0);
    }
}
