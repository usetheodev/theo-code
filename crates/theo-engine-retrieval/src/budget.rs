//! Token budget allocation for the GRAPHCTX pipeline.
//!
//! Divides a total token budget into named buckets using percentage allocation.
//! All percentages must sum to 1.0. Allocation uses floor division so the sum
//! never exceeds the input budget.

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Percentage-based budget configuration.
pub struct BudgetConfig {
    /// Fraction allocated to the repo-map layer (e.g. 0.15).
    pub repo_map_pct: f64,
    /// Fraction allocated to module community cards (e.g. 0.25).
    pub module_cards_pct: f64,
    /// Fraction allocated to verbatim source code (e.g. 0.40).
    pub real_code_pct: f64,
    /// Fraction allocated to task / conversation history (e.g. 0.15).
    pub task_history_pct: f64,
    /// Fraction held in reserve (e.g. 0.05).
    pub reserve_pct: f64,
}

/// Concrete token counts computed from a `BudgetConfig` and a total budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetAllocation {
    pub repo_map: usize,
    pub module_cards: usize,
    pub real_code: usize,
    pub task_history: usize,
    pub reserve: usize,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl BudgetConfig {
    /// The canonical 16 k-token budget split from the GRAPHCTX spec.
    pub fn default_16k() -> Self {
        BudgetConfig {
            repo_map_pct: 0.15,
            module_cards_pct: 0.25,
            real_code_pct: 0.40,
            task_history_pct: 0.15,
            reserve_pct: 0.05,
        }
    }

    /// Compute token counts for a given `total_budget`.
    ///
    /// Uses `floor` so the sum of all buckets never exceeds `total_budget`.
    pub fn allocate(&self, total_budget: usize) -> BudgetAllocation {
        let t = total_budget as f64;
        BudgetAllocation {
            repo_map: (t * self.repo_map_pct).floor() as usize,
            module_cards: (t * self.module_cards_pct).floor() as usize,
            real_code: (t * self.real_code_pct).floor() as usize,
            task_history: (t * self.task_history_pct).floor() as usize,
            reserve: (t * self.reserve_pct).floor() as usize,
        }
    }
}
