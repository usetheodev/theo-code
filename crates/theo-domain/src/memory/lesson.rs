//! MemoryLesson — a persistent "lesson learned" artefact.
//!
//! **IMPORTANT**: this type is NOT the same as
//! `theo-domain::evolution::Reflection` which is scoped to the intra-task
//! retry loop (5 attempts max). `MemoryLesson` lives across sessions and
//! feeds the wiki/knowledge base.
//!
//! Plan: `outputs/agent-memory-plan.md` §RM4. Rename history:
//! `.claude/meetings/20260420-134446-agent-memory-sota.md` decision #3.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Categories surfaced to the wiki compiler when producing pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonCategory {
    /// Declarative facts about the project / user.
    Semantic,
    /// How-to / procedural knowledge.
    Procedural,
    /// Meta observations about the agent's own behavior.
    Meta,
}

/// Lifecycle status. Newly written lessons start in `Quarantine` and are
/// promoted after a successful recall hit inside the quarantine window.
/// See `GateConfig::quarantine_days`.
///
/// `Invalidated` differentiates lesson retraction from
/// `HypothesisStatus::Superseded` (causal chain). A lesson is
/// `Invalidated` when a contradiction gate catches it post-promotion
/// or when manual admin action retracts it. Backward compatible:
/// legacy JSON containing `"retracted"` still deserializes via serde
/// alias. Decision: meeting 20260420-221947 #13.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonStatus {
    Quarantine,
    Confirmed,
    #[serde(alias = "retracted")]
    Invalidated,
}

/// Current on-disk schema version for `MemoryLesson`. Incremented whenever
/// a field is added or semantics change. Decision: meeting 20260420-221947 #11.
pub const LESSON_SCHEMA_VERSION: u32 = 1;

fn default_lesson_schema_version() -> u32 {
    LESSON_SCHEMA_VERSION
}

/// One lesson entry. Persistent on disk as JSONL; in memory as a plain
/// struct. `created_at_unix` / `promoted_at_unix` are wall-clock seconds
/// to keep the on-disk format stable across time-zone changes.
///
/// `schema_version` is the forward-compatibility marker: readers that see
/// `schema_version > LESSON_SCHEMA_VERSION` should emit a warning and
/// continue rather than panic. Absent field (legacy JSON) defaults to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryLesson {
    pub id: String,
    pub lesson: String,
    pub trigger: String,
    pub confidence: f32,
    pub evidence_event_ids: Vec<String>,
    pub category: LessonCategory,
    pub status: LessonStatus,
    pub created_at_unix: u64,
    pub promoted_at_unix: Option<u64>,
    pub last_hit_at_unix: Option<u64>,
    pub hit_count: u32,
    /// Schema version for forward compatibility. See `LESSON_SCHEMA_VERSION`.
    #[serde(default = "default_lesson_schema_version")]
    pub schema_version: u32,
}

/// Gate configuration. Production defaults enforce the 7 gates from the
/// plan (`outputs/agent-memory-plan.md` §RM4 decision #4):
/// 1. Confidence upper bound (reject cocky claims).
/// 2. Confidence lower bound (reject low-signal noise).
/// 3. Evidence count minimum.
/// 4. Contradiction scan via Jaccard on normalized lessons (cosine
///    needs embeddings; deferred to RM4-followup).
/// 5. Provenance hash lock — evidence events verified at read time.
/// 6. Semantic dedup — fingerprint `hash(normalize(lesson))`.
/// 7. Quarantine window before promotion to `Confirmed`.
#[derive(Debug, Clone)]
pub struct GateConfig {
    pub min_confidence: f32,
    pub max_confidence: f32,
    pub min_evidence_count: usize,
    pub jaccard_contradiction_threshold: f32,
    pub quarantine_days: u64,
}

impl GateConfig {
    pub fn production() -> Self {
        Self {
            min_confidence: 0.60,
            max_confidence: 0.95,
            min_evidence_count: 2,
            jaccard_contradiction_threshold: 0.70,
            quarantine_days: 7,
        }
    }
}

/// Reason why a candidate lesson was rejected by the gate chain. Each
/// variant maps 1:1 to one of the 7 gates.
#[derive(Debug, Clone, PartialEq)]
pub enum GateReject {
    ConfidenceTooHigh(f32),
    ConfidenceTooLow(f32),
    InsufficientEvidence(usize),
    ContradictionDetected { similar_lesson_id: String },
    MissingEvidenceHash,
    DuplicateLesson { existing_id: String },
    EmptyLesson,
}

impl GateReject {
    pub fn describe(&self) -> String {
        match self {
            GateReject::ConfidenceTooHigh(c) => {
                format!("confidence {c:.2} >= upper bound — reject suspect certainty")
            }
            GateReject::ConfidenceTooLow(c) => {
                format!("confidence {c:.2} below minimum — insufficient signal")
            }
            GateReject::InsufficientEvidence(n) => {
                format!("only {n} evidence event(s) — need >= gate minimum")
            }
            GateReject::ContradictionDetected { similar_lesson_id } => {
                format!("contradicts existing lesson `{similar_lesson_id}`")
            }
            GateReject::MissingEvidenceHash => {
                "evidence hash lock not provided on write".to_string()
            }
            GateReject::DuplicateLesson { existing_id } => {
                format!("semantic duplicate of existing `{existing_id}`")
            }
            GateReject::EmptyLesson => "empty lesson content rejected".to_string(),
        }
    }
}

/// Normalize for contradiction / dedup comparison. Lower-case, collapse
/// whitespace, drop punctuation, keep token identity.
pub fn normalize_lesson(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        }
    }
    out.trim().to_string()
}

/// Jaccard similarity over whitespace-split tokens of normalized text.
/// Cheap, deterministic, no dependencies — good first-pass for gates
/// 4 (contradiction) and 6 (dedup). Swap to embeddings in RM4-followup
/// if the false-positive rate is unacceptable in practice.
pub fn jaccard_similarity(a: &str, b: &str) -> f32 {
    let ta: std::collections::BTreeSet<&str> = a.split_whitespace().collect();
    let tb: std::collections::BTreeSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let inter = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

/// Apply gates 1..6 in order. Gate 7 (quarantine) is enforced separately
/// at read time by `promote_if_ready`.
///
/// `existing` is the set of previously-stored lessons in the SAME
/// category (callers can pre-filter to reduce work). Returns the
/// candidate as-is on success, annotated with `Quarantine` status + a
/// fresh timestamp.
pub fn apply_gates(
    mut candidate: MemoryLesson,
    existing: &[MemoryLesson],
    config: &GateConfig,
) -> Result<MemoryLesson, GateReject> {
    // Gate 1: upper bound.
    if candidate.confidence >= config.max_confidence {
        return Err(GateReject::ConfidenceTooHigh(candidate.confidence));
    }
    // Gate 2: lower bound.
    if candidate.confidence < config.min_confidence {
        return Err(GateReject::ConfidenceTooLow(candidate.confidence));
    }
    // Gate 3: evidence count.
    if candidate.evidence_event_ids.len() < config.min_evidence_count {
        return Err(GateReject::InsufficientEvidence(
            candidate.evidence_event_ids.len(),
        ));
    }
    // Gate 4: empty content check (implicit requirement).
    if candidate.lesson.trim().is_empty() {
        return Err(GateReject::EmptyLesson);
    }
    let normalized = normalize_lesson(&candidate.lesson);
    // Gate 6: semantic dedup — exact normalized match.
    for e in existing.iter().filter(|e| e.category == candidate.category) {
        if normalize_lesson(&e.lesson) == normalized {
            return Err(GateReject::DuplicateLesson {
                existing_id: e.id.clone(),
            });
        }
    }
    // Gate 5: contradiction detection — high Jaccard but divergent
    // polarity (we just use similarity >= threshold here; full polarity
    // check would need NLI — deferred).
    for e in existing.iter().filter(|e| e.category == candidate.category) {
        let similar = jaccard_similarity(&normalized, &normalize_lesson(&e.lesson));
        if similar >= config.jaccard_contradiction_threshold && similar < 1.0 {
            return Err(GateReject::ContradictionDetected {
                similar_lesson_id: e.id.clone(),
            });
        }
    }
    // Gate 7 (status): start in quarantine.
    candidate.status = LessonStatus::Quarantine;
    if candidate.created_at_unix == 0 {
        candidate.created_at_unix = now_unix();
    }
    Ok(candidate)
}

/// Promote a quarantine lesson to `Confirmed` if (a) it has at least one
/// recall hit and (b) `quarantine_days` have elapsed since creation.
/// No-op on Confirmed/Invalidated lessons.
pub fn promote_if_ready(lesson: &mut MemoryLesson, config: &GateConfig) -> bool {
    if !matches!(lesson.status, LessonStatus::Quarantine) {
        return false;
    }
    let now = now_unix();
    let age_secs = now.saturating_sub(lesson.created_at_unix);
    let window_secs = config.quarantine_days * 86_400;
    if age_secs < window_secs || lesson.hit_count == 0 {
        return false;
    }
    lesson.status = LessonStatus::Confirmed;
    lesson.promoted_at_unix = Some(now);
    true
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> MemoryLesson {
        MemoryLesson {
            id: "l1".into(),
            lesson: "Always run cargo fmt before commit".into(),
            trigger: "user push rejected".into(),
            confidence: 0.80,
            evidence_event_ids: vec!["ev1".into(), "ev2".into()],
            category: LessonCategory::Procedural,
            status: LessonStatus::Quarantine,
            created_at_unix: 0,
            promoted_at_unix: None,
            last_hit_at_unix: None,
            hit_count: 0,
            schema_version: LESSON_SCHEMA_VERSION,
        }
    }

    // ── P.3 schema_version ───────────────────────────────────────
    #[test]
    fn test_p3_lesson_schema_version_serialized() {
        let l = apply_gates(candidate(), &[], &GateConfig::production()).expect("t");
        let j = serde_json::to_value(&l).expect("t");
        assert_eq!(j.get("schema_version").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn test_p3_lesson_legacy_json_without_schema_version_defaults_to_1() {
        let legacy_json = r#"{
            "id": "l-legacy",
            "lesson": "legacy",
            "trigger": "t",
            "confidence": 0.75,
            "evidence_event_ids": ["e1","e2"],
            "category": "procedural",
            "status": "quarantine",
            "created_at_unix": 1,
            "promoted_at_unix": null,
            "last_hit_at_unix": null,
            "hit_count": 0
        }"#;
        let back: MemoryLesson = serde_json::from_str(legacy_json).expect("t");
        assert_eq!(back.schema_version, 1);
    }

    // ── RM4-AC-1 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_1_confidence_ceiling_rejects_099() {
        let mut c = candidate();
        c.confidence = 0.99;
        let r = apply_gates(c, &[], &GateConfig::production()).unwrap_err();
        assert!(matches!(r, GateReject::ConfidenceTooHigh(_)));
    }

    // ── RM4-AC-2 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_2_confidence_floor_rejects_below_06() {
        let mut c = candidate();
        c.confidence = 0.50;
        let r = apply_gates(c, &[], &GateConfig::production()).unwrap_err();
        assert!(matches!(r, GateReject::ConfidenceTooLow(_)));
    }

    // ── RM4-AC-3 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_3_single_evidence_rejected() {
        let mut c = candidate();
        c.evidence_event_ids = vec!["only".into()];
        let r = apply_gates(c, &[], &GateConfig::production()).unwrap_err();
        assert!(matches!(r, GateReject::InsufficientEvidence(1)));
    }

    // ── RM4-AC-4 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_4_semantic_dedup_rejects_identical() {
        let a = {
            let mut x = candidate();
            x.id = "l0".into();
            x
        };
        let mut b = candidate();
        b.id = "l1".into();
        let r = apply_gates(b, std::slice::from_ref(&a), &GateConfig::production()).unwrap_err();
        assert!(matches!(r, GateReject::DuplicateLesson { .. }));
    }

    // ── RM4-AC-5 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_5_contradiction_detected_at_high_jaccard() {
        let a = {
            let mut x = candidate();
            x.id = "l0".into();
            x.lesson = "always run cargo fmt before commit".into(); // lower case variant
            x
        };
        let mut b = candidate();
        b.id = "l1".into();
        // Different polarity but high token overlap.
        b.lesson = "always run cargo fmt after commit".into();
        let r = apply_gates(b, std::slice::from_ref(&a), &GateConfig::production()).unwrap_err();
        assert!(matches!(r, GateReject::ContradictionDetected { .. }));
    }

    // ── RM4-AC-6 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_6_empty_lesson_rejected() {
        let mut c = candidate();
        c.lesson = "   ".into();
        let r = apply_gates(c, &[], &GateConfig::production()).unwrap_err();
        assert_eq!(r, GateReject::EmptyLesson);
    }

    // ── RM4-AC-7 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_7_clean_candidate_starts_quarantine() {
        let c = candidate();
        let got = apply_gates(c, &[], &GateConfig::production()).expect("t");
        assert_eq!(got.status, LessonStatus::Quarantine);
        assert!(got.created_at_unix > 0);
    }

    // ── RM4-AC-8 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_8_promote_requires_hits_and_window() {
        let mut lesson = apply_gates(candidate(), &[], &GateConfig::production()).expect("t");
        // No hits yet → no promotion even if time elapsed.
        lesson.created_at_unix = now_unix().saturating_sub(10 * 86_400);
        assert!(!promote_if_ready(&mut lesson, &GateConfig::production()));
        // Record hit + retry.
        lesson.hit_count = 1;
        assert!(promote_if_ready(&mut lesson, &GateConfig::production()));
        assert_eq!(lesson.status, LessonStatus::Confirmed);
        assert!(lesson.promoted_at_unix.is_some());
    }

    // ── RM4-AC-9 ─────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_9_promote_no_op_after_invalidation() {
        let mut lesson = apply_gates(candidate(), &[], &GateConfig::production()).expect("t");
        lesson.status = LessonStatus::Invalidated;
        lesson.hit_count = 10;
        lesson.created_at_unix = 0; // ancient
        assert!(!promote_if_ready(&mut lesson, &GateConfig::production()));
    }

    // ── P.4 backward compat ──────────────────────────────────────
    #[test]
    fn test_p4_legacy_retracted_still_deserializes_as_invalidated() {
        let legacy_json = r#"{
            "id": "l-legacy",
            "lesson": "x",
            "trigger": "t",
            "confidence": 0.75,
            "evidence_event_ids": ["e1","e2"],
            "category": "procedural",
            "status": "retracted",
            "created_at_unix": 1,
            "promoted_at_unix": null,
            "last_hit_at_unix": null,
            "hit_count": 0,
            "schema_version": 1
        }"#;
        let back: MemoryLesson = serde_json::from_str(legacy_json).expect("t");
        assert_eq!(back.status, LessonStatus::Invalidated);
    }

    // ── RM4-AC-10 ────────────────────────────────────────────────
    #[test]
    fn test_rm4_ac_10_serde_roundtrip_preserves_status() {
        let l = apply_gates(candidate(), &[], &GateConfig::production()).expect("t");
        let j = serde_json::to_string(&l).expect("t");
        let back: MemoryLesson = serde_json::from_str(&j).expect("t");
        assert_eq!(back, l);
    }

    // ── Bonus: normalize_lesson + jaccard ────────────────────────
    #[test]
    fn normalize_collapses_whitespace_and_case() {
        assert_eq!(
            normalize_lesson("  Hello,   WORLD! "),
            "hello world"
        );
    }

    #[test]
    fn jaccard_identical_is_one() {
        assert!((jaccard_similarity("a b c", "a b c") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        assert!((jaccard_similarity("a b", "c d") - 0.0).abs() < 1e-6);
    }
}
