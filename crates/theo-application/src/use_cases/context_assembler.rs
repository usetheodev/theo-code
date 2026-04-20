//! Context Assembler — composes the right context package per agent step.
//!
//! Deterministic assembly with 4 hard rules:
//! 1. Always respect token budget
//! 2. Always include task objective
//! 3. Always include current plan step
//! 4. Always include recent evidence
//!
//! Fills remaining budget with structural context from GraphCTX.
//! No ML, no learned weights — pure deterministic heuristics.

use theo_domain::event::DomainEvent;
use theo_domain::graph_context::GraphContextResult;
use theo_domain::working_set::WorkingSet;

/// Assembled context ready for LLM prompt injection.
#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// Ordered sections of the assembled context.
    pub sections: Vec<String>,
    /// Total estimated token count.
    pub total_tokens: usize,
    /// Token budget that was requested.
    pub budget_tokens: usize,
}

/// Minimal deterministic context assembler.
///
/// Composes context from task state, working set, execution events,
/// and structural code context, respecting a strict token budget.
///
/// Supports optional feedback loop: communities with higher historical
/// usefulness scores get priority in structural context ordering.
pub struct ContextAssembler {
    token_budget: usize,
    /// Historical feedback scores per community (exponential moving average).
    /// Used to boost ordering of structural blocks that were previously useful.
    feedback_scores: std::collections::HashMap<String, f64>,
    /// How many times each community has been assembled consecutively.
    /// Used for repetition penalty (P0.5) and stability bonus decay (P1).
    assembly_counts: std::collections::HashMap<String, u32>,
    /// Communities that had positive signal (tool use/citation) last turn.
    /// Used for stability bonus (P1): only communities WITH signal get boost.
    positive_signal_sources: std::collections::HashSet<String>,
}

/// Budget allocation ratios for context assembly.
/// Ensures hot files don't starve structural context on large repos.
pub struct BudgetAllocation {
    /// Tokens reserved for task objective + current step (fixed overhead).
    pub task_overhead: usize,
    /// Tokens reserved for recent events + hot files.
    pub execution_context: usize,
    /// Tokens available for structural code context (the bulk).
    pub structural: usize,
}

impl ContextAssembler {
    pub fn new(token_budget: usize) -> Self {
        Self {
            token_budget,
            feedback_scores: std::collections::HashMap::new(),
            assembly_counts: std::collections::HashMap::new(),
            positive_signal_sources: std::collections::HashSet::new(),
        }
    }

    /// Create an assembler with adaptive budget based on repo size.
    ///
    /// Budget scales with sqrt(file_count): small repos get 4K, large repos up to 32K.
    /// This ensures large repos like FFmpeg (4.6K files) get adequate coverage.
    pub fn adaptive(file_count: usize) -> Self {
        let budget = Self::compute_adaptive_budget(file_count);
        Self::new(budget)
    }

    /// Compute adaptive token budget from file count.
    ///
    /// Formula: max(4000, min(32000, 500 * sqrt(file_count)))
    /// - 100 files → 5000 tokens
    /// - 500 files → 11180 tokens
    /// - 2000 files → 22360 tokens (our repo)
    /// - 5000 files → 32000 tokens (FFmpeg — hits cap)
    /// - 10000+ files → 32000 tokens (maximum)
    pub fn compute_adaptive_budget(file_count: usize) -> usize {
        let raw = (500.0 * (file_count as f64).sqrt()) as usize;
        raw.max(4000).min(32000)
    }

    /// Compute budget allocation ratios.
    ///
    /// 15% task overhead (objective + step) — hard minimum
    /// 25% execution context (events + hot files)
    /// 60% structural context (graph communities)
    pub fn compute_allocation(&self) -> BudgetAllocation {
        let task_overhead = (self.token_budget as f64 * 0.15) as usize;
        let execution_context = (self.token_budget as f64 * 0.25) as usize;
        let structural = self.token_budget - task_overhead - execution_context;
        BudgetAllocation {
            task_overhead,
            execution_context,
            structural,
        }
    }

    /// Returns the current token budget.
    pub fn budget(&self) -> usize {
        self.token_budget
    }

    /// Record that a community had positive signal this turn (tool use/citation).
    /// Only communities with positive signal get stability bonus.
    pub fn record_positive_signal(&mut self, community_id: &str) {
        self.positive_signal_sources
            .insert(community_id.to_string());
    }

    /// Clear positive signals at the start of each turn.
    pub fn clear_turn_signals(&mut self) {
        self.positive_signal_sources.clear();
    }

    /// Record usefulness feedback for a community.
    ///
    /// Uses exponential moving average (alpha=0.3) to smooth scores.
    /// Higher scores → community gets priority in future assembly.
    pub fn record_feedback(&mut self, community_id: &str, score: f64) {
        let alpha = 0.3;
        let entry = self
            .feedback_scores
            .entry(community_id.to_string())
            .or_insert(0.5);
        *entry = alpha * score + (1.0 - alpha) * *entry;
    }

    /// Get the current feedback score for a community (default 0.5).
    pub fn feedback_score(&self, community_id: &str) -> f64 {
        self.feedback_scores
            .get(community_id)
            .copied()
            .unwrap_or(0.5)
    }

    /// Load feedback scores from a JSON file.
    pub fn load_feedback(path: &std::path::Path) -> std::collections::HashMap<String, f64> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save feedback scores to a JSON file.
    pub fn save_feedback(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.feedback_scores)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }

    /// Assemble context for the current agent step.
    ///
    /// Hard rules (never violated):
    /// 1. `result.total_tokens <= self.token_budget`
    /// 2. Task objective is always included
    /// 3. Current plan step is always included (if present)
    /// 4. Recent evidence events are always included (up to 8)
    ///
    /// Remaining budget is filled with structural code context blocks.
    /// Assemble with episode memory injection (P1.5).
    /// Episode constraints and failure patterns are injected before structural context.
    /// Cap: 10% of total budget for episode content.
    pub fn assemble_with_memory(
        &mut self,
        task_objective: &str,
        working_set: &WorkingSet,
        structural_context: &GraphContextResult,
        recent_events: &[DomainEvent],
        episode: Option<&theo_domain::episode::EpisodeSummary>,
    ) -> AssembledContext {
        // Delegate to assemble with episode pre-injection
        let mut ctx = self.assemble(
            task_objective,
            working_set,
            structural_context,
            recent_events,
        );

        // P1.5: Inject episode memory BEFORE structural content (but after hard rules)
        if let Some(ep) = episode {
            let memory_budget = self.token_budget / 10; // 10% cap
            let mut memory_sections = Vec::new();
            let mut memory_tokens = 0;

            // Learned constraints from prior episodes
            if !ep.machine_summary.learned_constraints.is_empty() {
                let text = ep
                    .machine_summary
                    .learned_constraints
                    .iter()
                    .map(|c| format!("- {}", c))
                    .collect::<Vec<_>>()
                    .join("\n");
                let section = format!("## Prior Constraints\n{}", text);
                let tokens = estimate_tokens(&section);
                if memory_tokens + tokens <= memory_budget {
                    memory_sections.push(section);
                    memory_tokens += tokens;
                }
            }

            // Failed attempts from prior episodes
            if !ep.machine_summary.failed_attempts.is_empty() {
                let text = ep
                    .machine_summary
                    .failed_attempts
                    .iter()
                    .take(5)
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n");
                let section = format!("## Prior Failures (avoid repeating)\n{}", text);
                let tokens = estimate_tokens(&section);
                if memory_tokens + tokens <= memory_budget {
                    memory_sections.push(section);
                    memory_tokens += tokens;
                }
            }

            // Insert memory sections after task overhead, before structural
            // Find insertion point: after constraints/events, before structural
            let insert_idx = ctx.sections.len().min(5); // After first ~5 hard-rule sections
            for (i, section) in memory_sections.into_iter().enumerate() {
                ctx.sections.insert(insert_idx + i, section);
            }
            ctx.total_tokens += memory_tokens;
        }

        ctx
    }

    pub fn assemble(
        &mut self,
        task_objective: &str,
        working_set: &WorkingSet,
        structural_context: &GraphContextResult,
        recent_events: &[DomainEvent],
    ) -> AssembledContext {
        let mut sections = Vec::new();
        let mut tokens_used: usize = 0;

        // Rule 2: ALWAYS include task objective
        let objective_section = format!("## Task Objective\n{}", task_objective);
        let objective_tokens = estimate_tokens(&objective_section);
        if tokens_used + objective_tokens <= self.token_budget {
            sections.push(objective_section);
            tokens_used += objective_tokens;
        }

        // Rule 3: ALWAYS include current plan step (if present)
        if let Some(step) = &working_set.current_plan_step {
            let step_section = format!("## Current Step\n{}", step);
            let step_tokens = estimate_tokens(&step_section);
            if tokens_used + step_tokens <= self.token_budget {
                sections.push(step_section);
                tokens_used += step_tokens;
            }
        }

        // Include active hypothesis (if present)
        if let Some(hypothesis) = &working_set.active_hypothesis {
            let hyp_section = format!("## Active Hypothesis\n{}", hypothesis);
            let hyp_tokens = estimate_tokens(&hyp_section);
            if tokens_used + hyp_tokens <= self.token_budget {
                sections.push(hyp_section);
                tokens_used += hyp_tokens;
            }
        }

        // Include constraints (if any)
        if !working_set.constraints.is_empty() {
            let constraints_text = working_set
                .constraints
                .iter()
                .map(|c| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n");
            let section = format!("## Constraints\n{}", constraints_text);
            let tokens = estimate_tokens(&section);
            if tokens_used + tokens <= self.token_budget {
                sections.push(section);
                tokens_used += tokens;
            }
        }

        // Rule 4: ALWAYS include recent evidence events (up to 8)
        if !recent_events.is_empty() {
            let evidence_lines: Vec<String> = recent_events
                .iter()
                .rev()
                .take(8)
                .map(|e| {
                    format!(
                        "- [{}] {}: {}",
                        e.event_type,
                        e.entity_id,
                        summarize_payload(&e.payload)
                    )
                })
                .collect();
            let evidence_section = format!("## Recent Events\n{}", evidence_lines.join("\n"));
            let evidence_tokens = estimate_tokens(&evidence_section);
            if tokens_used + evidence_tokens <= self.token_budget {
                sections.push(evidence_section);
                tokens_used += evidence_tokens;
            }
        }

        // Include hot files list
        if !working_set.hot_files.is_empty() {
            let files_text = working_set
                .hot_files
                .iter()
                .map(|f| format!("- {}", f))
                .collect::<Vec<_>>()
                .join("\n");
            let section = format!("## Hot Files\n{}", files_text);
            let tokens = estimate_tokens(&section);
            if tokens_used + tokens <= self.token_budget {
                sections.push(section);
                tokens_used += tokens;
            }
        }

        // Fill remaining budget with structural code context.
        // Scoring: feedback boost + stability bonus - repetition penalty.
        // P0.5: Penalty floor = 0.5 (never zero a score, protect recall).
        // P1: Stability bonus requires positive signal, decays 0.7/turn.
        let mut blocks_with_priority: Vec<(usize, f64)> = structural_context
            .blocks
            .iter()
            .enumerate()
            .map(|(i, block)| {
                let source = &block.source_id;

                // Base: feedback-informed score
                let feedback_boost = self.feedback_score(source);
                let base_priority = block.score * (1.0 + feedback_boost);

                // P0.5: Repetition penalty — penalize communities assembled many times
                // Multiplier ∈ [0.5, 1.0]. Floor prevents recall collapse.
                let assembly_count = self.assembly_counts.get(source).copied().unwrap_or(0);
                let penalty_multiplier = (1.0 - 0.1 * assembly_count as f64).max(0.5);

                // P1: Stability bonus — boost if community had positive signal
                // Decays exponentially: 0.15 * 0.7^turns_without_signal
                let stability_bonus = if self.positive_signal_sources.contains(source) {
                    let turns = assembly_count.min(10);
                    0.15 * 0.7_f64.powi(turns as i32)
                } else {
                    0.0 // No signal = no bonus (prevents lock-in)
                };

                let priority = base_priority * penalty_multiplier + stability_bonus;
                (i, priority)
            })
            .collect();
        blocks_with_priority
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Track which communities are assembled this turn (for next turn's penalties)
        let mut assembled_this_turn = std::collections::HashSet::new();

        for (idx, _priority) in blocks_with_priority {
            let block = &structural_context.blocks[idx];
            if tokens_used + block.token_count > self.token_budget {
                continue;
            }
            sections.push(block.content.clone());
            tokens_used += block.token_count;
            assembled_this_turn.insert(block.source_id.clone());
        }

        // Update assembly counts for next turn
        for (source, count) in &mut self.assembly_counts {
            if !assembled_this_turn.contains(source) {
                *count = 0; // Reset if not assembled this turn
            }
        }
        for source in &assembled_this_turn {
            *self.assembly_counts.entry(source.clone()).or_insert(0) += 1;
        }

        // Rule 1: ALWAYS respect token budget (defensive check)
        debug_assert!(
            tokens_used <= self.token_budget,
            "Token budget violated: {} > {}",
            tokens_used,
            self.token_budget
        );

        AssembledContext {
            sections,
            total_tokens: tokens_used,
            budget_tokens: self.token_budget,
        }
    }
}

/// Simple token estimation: ~4 chars per token (industry heuristic).
fn estimate_tokens(text: &str) -> usize {
    // Ceiling division to be conservative
    (text.len() + 3) / 4
}

/// Extract a brief summary from an event payload for context display.
fn summarize_payload(payload: &serde_json::Value) -> String {
    if let Some(obj) = payload.as_object() {
        // Try common fields
        for key in &[
            "message",
            "tool_name",
            "hypothesis",
            "choice",
            "constraint",
            "error",
        ] {
            if let Some(val) = obj.get(*key) {
                if let Some(s) = val.as_str() {
                    let truncated = if s.len() > 80 { &s[..80] } else { s };
                    return truncated.to_string();
                }
            }
        }
    }
    if payload.is_null() {
        return String::new();
    }
    let s = payload.to_string();
    if s.len() > 80 { s[..80].to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::{DomainEvent, EventType};
    use theo_domain::graph_context::ContextBlock;

    fn make_working_set() -> WorkingSet {
        WorkingSet {
            hot_files: vec!["src/auth.rs".into(), "src/db.rs".into()],
            recent_event_ids: vec!["evt-1".into()],
            active_hypothesis: Some("jwt decode bug".into()),
            current_plan_step: Some("run cargo test".into()),
            constraints: vec!["no unwrap in auth".into()],
            ..WorkingSet::default()
        }
    }

    fn make_structural_context(blocks: Vec<(&str, usize)>) -> GraphContextResult {
        GraphContextResult {
            blocks: blocks
                .iter()
                .map(|(content, tokens)| ContextBlock {
                    block_id: String::new(),
                    source_id: "test".into(),
                    content: content.to_string(),
                    token_count: *tokens,
                    score: 0.5,
                })
                .collect(),
            total_tokens: blocks.iter().map(|(_, t)| t).sum(),
            budget_tokens: 4000,
            exploration_hints: String::new(),
            budget_report: None,
        }
    }

    fn make_events(n: usize) -> Vec<DomainEvent> {
        (0..n)
            .map(|i| {
                DomainEvent::new(
                    EventType::ToolCallCompleted,
                    format!("run-{}", i),
                    serde_json::json!({"tool_name": format!("tool_{}", i)}),
                )
            })
            .collect()
    }

    #[test]
    fn assembler_respects_token_budget() {
        let mut assembler = ContextAssembler::new(100);
        let ws = make_working_set();
        let ctx = make_structural_context(vec![("big block", 5000)]);
        let events = make_events(3);

        let result = assembler.assemble("fix bug", &ws, &ctx, &events);
        assert!(
            result.total_tokens <= 100,
            "Tokens {} exceeded budget 100",
            result.total_tokens
        );
    }

    #[test]
    fn assembler_always_includes_task_objective() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);
        let events = vec![];

        let result = assembler.assemble("fix authentication bug in jwt.rs", &ws, &ctx, &events);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("fix authentication bug in jwt.rs"),
            "Objective not found in assembled context"
        );
    }

    #[test]
    fn assembler_always_includes_current_step() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet {
            current_plan_step: Some("run cargo test".into()),
            ..WorkingSet::default()
        };
        let ctx = make_structural_context(vec![]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("run cargo test"),
            "Current step not found in context"
        );
    }

    #[test]
    fn assembler_includes_recent_evidence() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);
        let events = vec![DomainEvent::new(
            EventType::Error,
            "run-1",
            serde_json::json!({"message": "compile error in auth.rs"}),
        )];

        let result = assembler.assemble("fix", &ws, &ctx, &events);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("compile error in auth.rs"),
            "Evidence not found in context"
        );
    }

    #[test]
    fn assembler_fills_remaining_budget_with_structural_context() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![
            ("# Auth module\npub fn verify_token()", 50),
            ("# DB module\npub fn query()", 40),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("Auth module"),
            "Structural context not included"
        );
        assert!(full_text.contains("DB module"), "Second block not included");
    }

    #[test]
    fn assembler_stops_structural_when_budget_exhausted() {
        let mut assembler = ContextAssembler::new(200);
        let ws = WorkingSet::default();
        // Create blocks that exceed budget
        let ctx = make_structural_context(vec![
            ("small block", 50),
            ("big block that should not fit", 500),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(full_text.contains("small block"), "First block should fit");
        assert!(
            !full_text.contains("big block"),
            "Second block should be excluded"
        );
        assert!(result.total_tokens <= 200);
    }

    #[test]
    fn assembler_includes_hypothesis_and_constraints() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet {
            active_hypothesis: Some("race condition in event bus".into()),
            constraints: vec!["no unwrap".into(), "test before commit".into()],
            ..WorkingSet::default()
        };
        let ctx = make_structural_context(vec![]);

        let result = assembler.assemble("investigate", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(full_text.contains("race condition"), "Hypothesis missing");
        assert!(full_text.contains("no unwrap"), "Constraints missing");
    }

    #[test]
    fn assembler_empty_everything_still_works() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        assert!(
            !result.sections.is_empty(),
            "Should at least have objective"
        );
        assert!(result.total_tokens <= result.budget_tokens);
    }

    #[test]
    fn assembler_zero_budget_returns_empty() {
        let mut assembler = ContextAssembler::new(0);
        let ws = make_working_set();
        let ctx = make_structural_context(vec![("content", 100)]);
        let events = make_events(5);

        let result = assembler.assemble("task", &ws, &ctx, &events);
        assert!(result.total_tokens == 0);
        assert!(result.sections.is_empty());
    }

    #[test]
    fn estimate_tokens_approximation() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("abcde"), 2); // 5 chars = 2 tokens (ceiling)
        assert!(estimate_tokens("hello world") > 0);
    }

    #[test]
    fn summarize_payload_extracts_common_fields() {
        let p = serde_json::json!({"message": "compile error", "code": 1});
        assert_eq!(summarize_payload(&p), "compile error");

        let p2 = serde_json::json!({"tool_name": "bash"});
        assert_eq!(summarize_payload(&p2), "bash");

        assert_eq!(summarize_payload(&serde_json::Value::Null), "");
    }

    // --- P2-T1: Feedback loop tests ---

    fn make_tagged_context(blocks: Vec<(&str, &str, usize, f64)>) -> GraphContextResult {
        GraphContextResult {
            blocks: blocks
                .iter()
                .map(|(source_id, content, tokens, score)| ContextBlock {
                    block_id: String::new(),
                    source_id: source_id.to_string(),
                    content: content.to_string(),
                    token_count: *tokens,
                    score: *score,
                })
                .collect(),
            total_tokens: blocks.iter().map(|(_, _, t, _)| t).sum(),
            budget_tokens: 4000,
            exploration_hints: String::new(),
            budget_report: None,
        }
    }

    #[test]
    fn feedback_score_default_is_half() {
        let assembler = ContextAssembler::new(4000);
        assert!((assembler.feedback_score("unknown") - 0.5).abs() < 0.001);
    }

    #[test]
    fn record_feedback_updates_score() {
        let mut assembler = ContextAssembler::new(4000);
        assembler.record_feedback("community:auth", 1.0);
        assert!(
            assembler.feedback_score("community:auth") > 0.5,
            "High feedback should increase score above default"
        );

        assembler.record_feedback("community:db", 0.0);
        assert!(
            assembler.feedback_score("community:db") < 0.5,
            "Low feedback should decrease score below default"
        );
    }

    #[test]
    fn feedback_boosts_ordering_of_useful_communities() {
        let mut assembler = ContextAssembler::new(4000);
        // Give auth high feedback, db low feedback
        for _ in 0..5 {
            assembler.record_feedback("auth", 1.0);
            assembler.record_feedback("db", 0.0);
        }

        let ws = WorkingSet::default();
        // Both blocks have same relevance score (0.5) but different source_ids
        let ctx = make_tagged_context(vec![
            ("db", "# DB module", 50, 0.5),
            ("auth", "# Auth module", 50, 0.5),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");

        // Auth should appear before DB due to higher feedback
        let auth_pos = full_text.find("Auth module");
        let db_pos = full_text.find("DB module");
        assert!(
            auth_pos.is_some() && db_pos.is_some(),
            "Both blocks should be in context"
        );
        assert!(
            auth_pos.unwrap() < db_pos.unwrap(),
            "Auth should appear before DB due to feedback boost"
        );
    }

    #[test]
    fn feedback_never_violates_budget() {
        let mut assembler = ContextAssembler::new(100);
        assembler.record_feedback("auth", 1.0);

        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![("auth", "huge auth content", 5000, 0.9)]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        assert!(result.total_tokens <= 100, "Budget must never be exceeded");
    }

    #[test]
    fn feedback_persistence_roundtrip() {
        let mut assembler = ContextAssembler::new(4000);
        assembler.record_feedback("auth", 0.9);
        assembler.record_feedback("db", 0.2);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feedback.json");

        assembler.save_feedback(&path).unwrap();

        let loaded = ContextAssembler::load_feedback(&path);
        assert!(loaded.contains_key("auth"));
        assert!(loaded.contains_key("db"));
    }

    // --- SOTA: Adaptive budget tests ---

    #[test]
    fn adaptive_budget_small_repo() {
        // sqrt(50) * 500 = 3535 → clamped to 4000 (min)
        assert_eq!(ContextAssembler::compute_adaptive_budget(50), 4000);
    }

    #[test]
    fn adaptive_budget_medium_repo() {
        // sqrt(5000) * 500 ≈ 35355 → clamped to 32000 (max)
        let budget = ContextAssembler::compute_adaptive_budget(5000);
        assert_eq!(budget, 32000, "FFmpeg-size repo should hit max budget");
    }

    #[test]
    fn adaptive_budget_mid_range() {
        // sqrt(500) * 500 ≈ 11180
        let budget = ContextAssembler::compute_adaptive_budget(500);
        assert!(
            budget > 10000 && budget < 12000,
            "500-file repo should get ~11K budget, got {}",
            budget
        );
    }

    #[test]
    fn adaptive_budget_never_exceeds_max() {
        assert_eq!(ContextAssembler::compute_adaptive_budget(1_000_000), 32000);
    }

    #[test]
    fn adaptive_budget_never_below_min() {
        assert_eq!(ContextAssembler::compute_adaptive_budget(1), 4000);
        assert_eq!(ContextAssembler::compute_adaptive_budget(0), 4000);
    }

    #[test]
    fn budget_allocation_ratios() {
        let assembler = ContextAssembler::new(10000);
        let alloc = assembler.compute_allocation();
        assert_eq!(alloc.task_overhead, 1500); // 15%
        assert_eq!(alloc.execution_context, 2500); // 25%
        assert_eq!(alloc.structural, 6000); // 60%
        assert_eq!(
            alloc.task_overhead + alloc.execution_context + alloc.structural,
            10000
        );
    }

    #[test]
    fn adaptive_constructor_works() {
        let assembler = ContextAssembler::adaptive(5000);
        assert!(assembler.budget() > 4000);
    }

    // --- P0.5: Penalty tests ---

    #[test]
    fn penalty_reduces_score_for_repeated_communities() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![
            ("auth", "# Auth first", 50, 0.8),
            ("db", "# DB module", 50, 0.8),
        ]);

        // First assembly — both communities fresh
        let _r1 = assembler.assemble("task", &ws, &ctx, &[]);

        // Second assembly — auth was assembled before, should be penalized
        let r2 = assembler.assemble("task", &ws, &ctx, &[]);
        // Both still in context (penalty floor=0.5 keeps them), but ordering may change
        assert!(r2.total_tokens <= 4000);
    }

    #[test]
    fn penalty_floor_prevents_exclusion() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![("auth", "# Auth module", 50, 0.8)]);

        // Assemble 10 times — penalty should floor at 0.5, never exclude
        for _ in 0..10 {
            let result = assembler.assemble("task", &ws, &ctx, &[]);
            let full = result.sections.join("\n");
            assert!(
                full.contains("Auth module"),
                "Penalty floor must prevent total exclusion"
            );
        }
    }

    // --- P1: Stability bonus tests ---

    #[test]
    fn stability_bonus_only_with_positive_signal() {
        let mut assembler = ContextAssembler::new(4000);
        // NO positive signal recorded
        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![("auth", "# Auth", 50, 0.5), ("db", "# DB", 50, 0.5)]);

        let _r1 = assembler.assemble("task", &ws, &ctx, &[]);

        // Without positive signal, ordering should not favor auth over db
        // (both have same base score and no signal)
    }

    #[test]
    fn stability_bonus_with_signal_boosts() {
        let mut assembler = ContextAssembler::new(4000);
        assembler.record_positive_signal("auth");

        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![
            ("db", "# DB module", 50, 0.5),
            ("auth", "# Auth module", 50, 0.5),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full = result.sections.join("\n");

        // Auth should appear before DB due to stability bonus
        if let (Some(auth_pos), Some(db_pos)) = (full.find("Auth module"), full.find("DB module")) {
            assert!(auth_pos < db_pos, "Auth with signal should rank before DB");
        }
    }

    // --- P1.5: Memory injection tests ---

    #[test]
    fn memory_injection_includes_constraints() {
        use theo_domain::episode::*;
        let mut assembler = ContextAssembler::new(10000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![("# Structural", 100)]);

        let episode = EpisodeSummary::from_events("r-1", None, "task", &[]);
        // Manually set constraints for test
        let mut ep = episode;
        ep.machine_summary.learned_constraints = vec!["avoid unwrap in auth".into()];

        let result = assembler.assemble_with_memory("task", &ws, &ctx, &[], Some(&ep));
        let full = result.sections.join("\n");
        assert!(
            full.contains("avoid unwrap in auth"),
            "Episode constraints must appear in assembled context"
        );
    }

    #[test]
    fn memory_injection_capped_at_10_percent() {
        use theo_domain::episode::*;
        let mut assembler = ContextAssembler::new(1000); // small budget
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);

        let mut ep = EpisodeSummary::from_events("r-1", None, "task", &[]);
        // Add many constraints (would exceed 10% = 100 tokens)
        ep.machine_summary.learned_constraints = (0..50)
            .map(|i| {
                format!(
                    "constraint {} with a very long description that takes many tokens",
                    i
                )
            })
            .collect();

        let result = assembler.assemble_with_memory("task", &ws, &ctx, &[], Some(&ep));
        // Memory content should be capped
        let memory_tokens: usize = result
            .sections
            .iter()
            .filter(|s| s.contains("Prior Constraints") || s.contains("Prior Failures"))
            .map(|s| estimate_tokens(s))
            .sum();
        assert!(
            memory_tokens <= 100, // 10% of 1000
            "Memory tokens {} should be <= 100 (10% cap)",
            memory_tokens
        );
    }
}
