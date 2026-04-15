use serde::{Deserialize, Serialize};

/// Budget limits for an agent execution.
///
/// Invariant 8: no execution can run without budget limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// Maximum wall-clock time in seconds.
    pub max_time_secs: u64,
    /// Maximum total tokens (prompt + completion).
    pub max_tokens: u64,
    /// Maximum iterations of the agent loop.
    pub max_iterations: usize,
    /// Maximum number of tool calls.
    pub max_tool_calls: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_time_secs: 3_600,  // 1 hour (Claude Code has no time limit)
            max_tokens: 1_000_000, // 1M tokens (Claude Code context window)
            max_iterations: 200,   // Effectively no practical limit
            max_tool_calls: 500,   // Generous tool call budget
        }
    }
}

/// Tracks resource consumption during an agent execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetUsage {
    pub elapsed_secs: u64,
    pub tokens_used: u64,
    pub iterations_used: usize,
    pub tool_calls_used: usize,
}

impl BudgetUsage {
    /// Returns the first budget violation found, or None if within limits.
    pub fn exceeds(&self, budget: &Budget) -> Option<BudgetViolation> {
        if self.elapsed_secs > budget.max_time_secs {
            Some(BudgetViolation::TimeExceeded {
                limit: budget.max_time_secs,
                actual: self.elapsed_secs,
            })
        } else if self.tokens_used > budget.max_tokens {
            Some(BudgetViolation::TokensExceeded {
                limit: budget.max_tokens,
                actual: self.tokens_used,
            })
        } else if self.iterations_used > budget.max_iterations {
            Some(BudgetViolation::IterationsExceeded {
                limit: budget.max_iterations,
                actual: self.iterations_used,
            })
        } else if self.tool_calls_used > budget.max_tool_calls {
            Some(BudgetViolation::ToolCallsExceeded {
                limit: budget.max_tool_calls,
                actual: self.tool_calls_used,
            })
        } else {
            None
        }
    }
}

/// The type of budget limit that was violated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetViolation {
    TimeExceeded { limit: u64, actual: u64 },
    TokensExceeded { limit: u64, actual: u64 },
    IterationsExceeded { limit: usize, actual: usize },
    ToolCallsExceeded { limit: usize, actual: usize },
}

/// Per-model pricing (cost per million tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_write_per_million: f64,
}

/// Breakdown of dollar costs for a request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

impl CostBreakdown {
    pub fn calculate(cost: &ModelCost, input_tokens: u64, output_tokens: u64) -> Self {
        let input = (cost.input_per_million / 1_000_000.0) * input_tokens as f64;
        let output = (cost.output_per_million / 1_000_000.0) * output_tokens as f64;
        Self {
            input,
            output,
            cache_read: 0.0,
            cache_write: 0.0,
            total: input + output,
        }
    }

    /// Calculate with cache token breakdown.
    pub fn calculate_with_cache(
        cost: &ModelCost,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> Self {
        let input = (cost.input_per_million / 1_000_000.0) * input_tokens as f64;
        let output = (cost.output_per_million / 1_000_000.0) * output_tokens as f64;
        let cache_read = (cost.cache_read_per_million / 1_000_000.0) * cache_read_tokens as f64;
        let cache_write = (cost.cache_write_per_million / 1_000_000.0) * cache_write_tokens as f64;
        Self {
            input,
            output,
            cache_read,
            cache_write,
            total: input + output + cache_read + cache_write,
        }
    }
}

/// Known model costs (approximate, for display only).
pub fn known_model_cost(model: &str) -> Option<ModelCost> {
    match model {
        m if m.contains("gpt-4o") => Some(ModelCost {
            input_per_million: 2.5,
            output_per_million: 10.0,
            cache_read_per_million: 1.25,
            cache_write_per_million: 0.0,
        }),
        m if m.contains("gpt-4.1") => Some(ModelCost {
            input_per_million: 2.0,
            output_per_million: 8.0,
            cache_read_per_million: 0.5,
            cache_write_per_million: 0.0,
        }),
        m if m.contains("claude-sonnet-4") => Some(ModelCost {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_read_per_million: 0.3,
            cache_write_per_million: 3.75,
        }),
        m if m.contains("claude-opus") => Some(ModelCost {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_read_per_million: 1.5,
            cache_write_per_million: 18.75,
        }),
        _ => None,
    }
}

impl std::fmt::Display for BudgetViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetViolation::TimeExceeded { limit, actual } => {
                write!(f, "time exceeded: {}s > {}s limit", actual, limit)
            }
            BudgetViolation::TokensExceeded { limit, actual } => {
                write!(f, "tokens exceeded: {} > {} limit", actual, limit)
            }
            BudgetViolation::IterationsExceeded { limit, actual } => {
                write!(f, "iterations exceeded: {} > {} limit", actual, limit)
            }
            BudgetViolation::ToolCallsExceeded { limit, actual } => {
                write!(f, "tool calls exceeded: {} > {} limit", actual, limit)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budget_has_sensible_values() {
        let b = Budget::default();
        assert_eq!(b.max_time_secs, 3_600);
        assert_eq!(b.max_tokens, 1_000_000);
        assert_eq!(b.max_iterations, 200);
        assert_eq!(b.max_tool_calls, 500);
    }

    #[test]
    fn exceeds_returns_none_when_within_limits() {
        let budget = Budget::default();
        let usage = BudgetUsage {
            elapsed_secs: 10,
            tokens_used: 1000,
            iterations_used: 5,
            tool_calls_used: 10,
        };
        assert!(usage.exceeds(&budget).is_none());
    }

    #[test]
    fn exceeds_returns_time_exceeded() {
        let budget = Budget {
            max_time_secs: 60,
            ..Budget::default()
        };
        let usage = BudgetUsage {
            elapsed_secs: 61,
            ..Default::default()
        };
        let violation = usage.exceeds(&budget).unwrap();
        assert!(matches!(
            violation,
            BudgetViolation::TimeExceeded {
                limit: 60,
                actual: 61
            }
        ));
    }

    #[test]
    fn exceeds_returns_tokens_exceeded() {
        let budget = Budget {
            max_tokens: 1000,
            ..Budget::default()
        };
        let usage = BudgetUsage {
            tokens_used: 1001,
            ..Default::default()
        };
        let violation = usage.exceeds(&budget).unwrap();
        assert!(matches!(
            violation,
            BudgetViolation::TokensExceeded {
                limit: 1000,
                actual: 1001
            }
        ));
    }

    #[test]
    fn exceeds_returns_iterations_exceeded() {
        let budget = Budget {
            max_iterations: 10,
            ..Budget::default()
        };
        let usage = BudgetUsage {
            iterations_used: 11,
            ..Default::default()
        };
        let violation = usage.exceeds(&budget).unwrap();
        assert!(matches!(
            violation,
            BudgetViolation::IterationsExceeded {
                limit: 10,
                actual: 11
            }
        ));
    }

    #[test]
    fn exceeds_returns_tool_calls_exceeded() {
        let budget = Budget {
            max_tool_calls: 5,
            ..Budget::default()
        };
        let usage = BudgetUsage {
            tool_calls_used: 6,
            ..Default::default()
        };
        let violation = usage.exceeds(&budget).unwrap();
        assert!(matches!(
            violation,
            BudgetViolation::ToolCallsExceeded {
                limit: 5,
                actual: 6
            }
        ));
    }

    #[test]
    fn budget_violation_serde_roundtrip() {
        let violations = [
            BudgetViolation::TimeExceeded {
                limit: 300,
                actual: 350,
            },
            BudgetViolation::TokensExceeded {
                limit: 200_000,
                actual: 250_000,
            },
            BudgetViolation::IterationsExceeded {
                limit: 30,
                actual: 31,
            },
            BudgetViolation::ToolCallsExceeded {
                limit: 100,
                actual: 101,
            },
        ];
        for v in &violations {
            let json = serde_json::to_string(v).unwrap();
            let back: BudgetViolation = serde_json::from_str(&json).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn budget_serde_roundtrip() {
        let budget = Budget::default();
        let json = serde_json::to_string(&budget).unwrap();
        let back: Budget = serde_json::from_str(&json).unwrap();
        assert_eq!(back.max_time_secs, budget.max_time_secs);
        assert_eq!(back.max_tokens, budget.max_tokens);
    }

    #[test]
    fn display_budget_violation() {
        let v = BudgetViolation::TimeExceeded {
            limit: 300,
            actual: 350,
        };
        assert_eq!(format!("{}", v), "time exceeded: 350s > 300s limit");
    }

    #[test]
    fn cost_breakdown_calculate_basic() {
        // Arrange
        let cost = ModelCost {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_read_per_million: 0.3,
            cache_write_per_million: 3.75,
        };

        // Act
        let breakdown = CostBreakdown::calculate(&cost, 1_000_000, 500_000);

        // Assert
        assert!((breakdown.input - 3.0).abs() < 1e-9, "1M input tokens at $3/M = $3");
        assert!((breakdown.output - 7.5).abs() < 1e-9, "500K output tokens at $15/M = $7.5");
        assert_eq!(breakdown.cache_read, 0.0);
        assert_eq!(breakdown.cache_write, 0.0);
        assert!((breakdown.total - 10.5).abs() < 1e-9);
    }

    #[test]
    fn cost_breakdown_calculate_zero_tokens() {
        // Arrange
        let cost = ModelCost {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_read_per_million: 0.3,
            cache_write_per_million: 3.75,
        };

        // Act
        let breakdown = CostBreakdown::calculate(&cost, 0, 0);

        // Assert
        assert_eq!(breakdown.total, 0.0);
    }

    #[test]
    fn cost_breakdown_calculate_with_cache() {
        // Arrange
        let cost = ModelCost {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_read_per_million: 0.3,
            cache_write_per_million: 3.75,
        };

        // Act
        let breakdown = CostBreakdown::calculate_with_cache(&cost, 100_000, 50_000, 200_000, 100_000);

        // Assert
        let expected_input = 3.0 / 1_000_000.0 * 100_000.0;
        let expected_output = 15.0 / 1_000_000.0 * 50_000.0;
        let expected_cache_read = 0.3 / 1_000_000.0 * 200_000.0;
        let expected_cache_write = 3.75 / 1_000_000.0 * 100_000.0;
        let expected_total = expected_input + expected_output + expected_cache_read + expected_cache_write;

        assert!((breakdown.input - expected_input).abs() < 1e-9);
        assert!((breakdown.output - expected_output).abs() < 1e-9);
        assert!((breakdown.cache_read - expected_cache_read).abs() < 1e-9);
        assert!((breakdown.cache_write - expected_cache_write).abs() < 1e-9);
        assert!((breakdown.total - expected_total).abs() < 1e-9);
    }

    #[test]
    fn known_model_cost_returns_some_for_known_models() {
        assert!(known_model_cost("gpt-4o-2024-08-06").is_some());
        assert!(known_model_cost("gpt-4.1-mini").is_some());
        assert!(known_model_cost("claude-sonnet-4-20260514").is_some());
        assert!(known_model_cost("claude-opus-4-20260514").is_some());
    }

    #[test]
    fn known_model_cost_returns_none_for_unknown_model() {
        assert!(known_model_cost("llama-3-70b").is_none());
        assert!(known_model_cost("unknown-model").is_none());
    }

    #[test]
    fn known_model_cost_gpt4o_pricing() {
        // Arrange & Act
        let cost = known_model_cost("gpt-4o").unwrap();

        // Assert
        assert!((cost.input_per_million - 2.5).abs() < 1e-9);
        assert!((cost.output_per_million - 10.0).abs() < 1e-9);
    }

    #[test]
    fn cost_breakdown_default_is_zero() {
        let breakdown = CostBreakdown::default();
        assert_eq!(breakdown.input, 0.0);
        assert_eq!(breakdown.output, 0.0);
        assert_eq!(breakdown.cache_read, 0.0);
        assert_eq!(breakdown.cache_write, 0.0);
        assert_eq!(breakdown.total, 0.0);
    }

    #[test]
    fn cost_breakdown_serde_roundtrip() {
        let cost = ModelCost {
            input_per_million: 3.0,
            output_per_million: 15.0,
            cache_read_per_million: 0.3,
            cache_write_per_million: 3.75,
        };
        let breakdown = CostBreakdown::calculate(&cost, 1_000_000, 500_000);
        let json = serde_json::to_string(&breakdown).unwrap();
        let back: CostBreakdown = serde_json::from_str(&json).unwrap();
        assert!((back.total - breakdown.total).abs() < 1e-9);
    }

    #[test]
    fn model_cost_serde_roundtrip() {
        let cost = ModelCost {
            input_per_million: 15.0,
            output_per_million: 75.0,
            cache_read_per_million: 1.5,
            cache_write_per_million: 18.75,
        };
        let json = serde_json::to_string(&cost).unwrap();
        let back: ModelCost = serde_json::from_str(&json).unwrap();
        assert!((back.input_per_million - 15.0).abs() < 1e-9);
        assert!((back.output_per_million - 75.0).abs() < 1e-9);
    }
}
