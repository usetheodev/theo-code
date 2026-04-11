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
}
