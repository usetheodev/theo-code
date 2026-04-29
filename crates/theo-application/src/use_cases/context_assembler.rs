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
        raw.clamp(4000, 32000)
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
    ///
    /// T2.7: uses the bounded deserialization helper so a corrupt or
    /// malicious feedback file cannot force unbounded allocation.
    pub fn load_feedback(path: &std::path::Path) -> std::collections::HashMap<String, f64> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| {
                theo_domain::safe_json::from_str_bounded(
                    &s,
                    theo_domain::safe_json::DEFAULT_JSON_LIMIT,
                )
                .ok()
            })
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

        push_section(
            &mut sections,
            &mut tokens_used,
            self.token_budget,
            format!("## Task Objective\n{task_objective}"),
        );
        if let Some(step) = &working_set.current_plan_step {
            push_section(
                &mut sections,
                &mut tokens_used,
                self.token_budget,
                format!("## Current Step\n{step}"),
            );
        }
        if let Some(hypothesis) = &working_set.active_hypothesis {
            push_section(
                &mut sections,
                &mut tokens_used,
                self.token_budget,
                format!("## Active Hypothesis\n{hypothesis}"),
            );
        }
        if !working_set.constraints.is_empty() {
            push_section(
                &mut sections,
                &mut tokens_used,
                self.token_budget,
                format!(
                    "## Constraints\n{}",
                    working_set
                        .constraints
                        .iter()
                        .map(|c| format!("- {c}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            );
        }
        if !recent_events.is_empty() {
            push_section(
                &mut sections,
                &mut tokens_used,
                self.token_budget,
                format_recent_events(recent_events),
            );
        }
        if !working_set.hot_files.is_empty() {
            push_section(
                &mut sections,
                &mut tokens_used,
                self.token_budget,
                format!(
                    "## Hot Files\n{}",
                    working_set
                        .hot_files
                        .iter()
                        .map(|f| format!("- {f}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            );
        }

        let assembled_this_turn =
            self.fill_with_structural_blocks(structural_context, &mut sections, &mut tokens_used);
        self.update_assembly_counts(&assembled_this_turn);

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

    /// Score, sort, and pack as many structural blocks as the remaining
    /// budget allows. Returns the set of community IDs actually assembled.
    fn fill_with_structural_blocks(
        &self,
        structural_context: &GraphContextResult,
        sections: &mut Vec<String>,
        tokens_used: &mut usize,
    ) -> std::collections::HashSet<String> {
        let mut blocks_with_priority: Vec<(usize, f64)> = structural_context
            .blocks
            .iter()
            .enumerate()
            .map(|(i, block)| (i, self.priority_for(block)))
            .collect();
        blocks_with_priority
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut assembled = std::collections::HashSet::new();
        for (idx, _priority) in blocks_with_priority {
            let block = &structural_context.blocks[idx];
            if *tokens_used + block.token_count > self.token_budget {
                continue;
            }
            sections.push(block.content.clone());
            *tokens_used += block.token_count;
            assembled.insert(block.source_id.clone());
        }
        assembled
    }

    /// Priority = (base_score × repetition_penalty) + stability_bonus.
    /// Floor 0.5 prevents recall collapse; bonus decays 0.7/turn.
    fn priority_for(&self, block: &theo_domain::graph_context::ContextBlock) -> f64 {
        let source = &block.source_id;
        let feedback_boost = self.feedback_score(source);
        let base_priority = block.score * (1.0 + feedback_boost);
        let assembly_count = self.assembly_counts.get(source).copied().unwrap_or(0);
        let penalty_multiplier = (1.0 - 0.1 * assembly_count as f64).max(0.5);
        let stability_bonus = if self.positive_signal_sources.contains(source) {
            let turns = assembly_count.min(10);
            0.15 * 0.7_f64.powi(turns as i32)
        } else {
            0.0
        };
        base_priority * penalty_multiplier + stability_bonus
    }

    /// Reset counters for communities NOT assembled this turn; bump those
    /// that were so the repetition penalty kicks in next turn.
    fn update_assembly_counts(&mut self, assembled_this_turn: &std::collections::HashSet<String>) {
        for (source, count) in &mut self.assembly_counts {
            if !assembled_this_turn.contains(source) {
                *count = 0;
            }
        }
        for source in assembled_this_turn {
            *self.assembly_counts.entry(source.clone()).or_insert(0) += 1;
        }
    }
}

/// Append `section` to `sections` if it fits in the remaining budget.
fn push_section(
    sections: &mut Vec<String>,
    tokens_used: &mut usize,
    budget: usize,
    section: String,
) {
    let tokens = estimate_tokens(&section);
    if *tokens_used + tokens <= budget {
        sections.push(section);
        *tokens_used += tokens;
    }
}

fn format_recent_events(recent_events: &[DomainEvent]) -> String {
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
    format!("## Recent Events\n{}", evidence_lines.join("\n"))
}

/// Simple token estimation: ~4 chars per token (industry heuristic).
fn estimate_tokens(text: &str) -> usize {
    // Ceiling division to be conservative
    text.len().div_ceil(4)
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
            if let Some(val) = obj.get(*key)
                && let Some(s) = val.as_str() {
                    let truncated = if s.len() > 80 { &s[..80] } else { s };
                    return truncated.to_string();
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
#[path = "context_assembler_tests.rs"]
mod tests;
