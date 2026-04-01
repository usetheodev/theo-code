/// Multi-Armed Bandit (UCB1) for adaptive token budget allocation.
///
/// Learns the optimal split of the context window across 4 content categories
/// (full source code, signatures, repo map, tests) per task type.
///
/// UCB1 formula: score = avg_reward + sqrt(2 * ln(total_pulls) / arm_pulls)
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Coarse task classification for budget allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskType {
    BugFix,
    FeatureAdd,
    Refactor,
    Understand,
    Unknown,
}

/// How the token budget should be split across content categories.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BudgetSplit {
    /// Percentage for full source code.
    pub code_pct: f64,
    /// Percentage for function/type signatures.
    pub signature_pct: f64,
    /// Percentage for repo map (file tree + summaries).
    pub map_pct: f64,
    /// Percentage for test files.
    pub tests_pct: f64,
}

/// UCB1 bandit that learns optimal token budget splits per task type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetBandit {
    /// Per task type → per arm: (total_reward, pull_count).
    arms: HashMap<TaskType, Vec<(f64, usize)>>,
}

// ---------------------------------------------------------------------------
// Arm configurations
// ---------------------------------------------------------------------------

/// The 4 pre-defined budget configurations (arms).
const ARMS: [BudgetSplit; 4] = [
    // Arm 0: code-heavy
    BudgetSplit {
        code_pct: 0.60,
        signature_pct: 0.15,
        map_pct: 0.15,
        tests_pct: 0.10,
    },
    // Arm 1: balanced
    BudgetSplit {
        code_pct: 0.40,
        signature_pct: 0.25,
        map_pct: 0.20,
        tests_pct: 0.15,
    },
    // Arm 2: interface-heavy
    BudgetSplit {
        code_pct: 0.25,
        signature_pct: 0.35,
        map_pct: 0.25,
        tests_pct: 0.15,
    },
    // Arm 3: test-heavy
    BudgetSplit {
        code_pct: 0.30,
        signature_pct: 0.20,
        map_pct: 0.15,
        tests_pct: 0.35,
    },
];

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl BudgetBandit {
    /// Create a new bandit with zero observations for all task types.
    pub fn new() -> Self {
        let mut arms = HashMap::new();
        for &task in &[
            TaskType::BugFix,
            TaskType::FeatureAdd,
            TaskType::Refactor,
            TaskType::Understand,
            TaskType::Unknown,
        ] {
            // (total_reward, pull_count) initialized to (0.0, 0) per arm.
            arms.insert(task, vec![(0.0, 0_usize); ARMS.len()]);
        }
        BudgetBandit { arms }
    }

    /// Select the best budget split for a task type using UCB1.
    ///
    /// Arms that have never been pulled are selected first (exploration).
    /// After all arms have been pulled at least once, UCB1 balances
    /// exploitation (high average reward) with exploration (uncertainty).
    pub fn select(&self, task_type: TaskType) -> BudgetSplit {
        let arm_stats = match self.arms.get(&task_type) {
            Some(stats) => stats,
            None => return ARMS[1], // fallback to balanced
        };

        let total_pulls: usize = arm_stats.iter().map(|(_, c)| c).sum();

        // If any arm has never been pulled, pick the first unpulled one.
        for (i, &(_, count)) in arm_stats.iter().enumerate() {
            if count == 0 {
                return ARMS[i];
            }
        }

        // UCB1 selection.
        let ln_total = (total_pulls as f64).ln();
        let mut best_arm = 0;
        let mut best_score = f64::NEG_INFINITY;

        for (i, &(total_reward, count)) in arm_stats.iter().enumerate() {
            let avg = total_reward / count as f64;
            let exploration = (2.0 * ln_total / count as f64).sqrt();
            let score = avg + exploration;

            if score > best_score {
                best_score = score;
                best_arm = i;
            }
        }

        ARMS[best_arm]
    }

    /// Update the bandit with observed reward for a specific arm.
    ///
    /// `arm_index` must be in 0..4 (number of arms).
    /// `reward` should be in [0.0, 1.0] where 0.0 = failure, 1.0 = success.
    pub fn update(&mut self, task_type: TaskType, arm_index: usize, reward: f64) {
        let arm_stats = self.arms.entry(task_type).or_insert_with(|| {
            vec![(0.0, 0_usize); ARMS.len()]
        });

        if arm_index < arm_stats.len() {
            arm_stats[arm_index].0 += reward;
            arm_stats[arm_index].1 += 1;
        }
    }

    /// Return the arm index for a given BudgetSplit (for reward feedback).
    pub fn arm_index_for(split: &BudgetSplit) -> Option<usize> {
        ARMS.iter().position(|a| {
            (a.code_pct - split.code_pct).abs() < f64::EPSILON
                && (a.signature_pct - split.signature_pct).abs() < f64::EPSILON
                && (a.map_pct - split.map_pct).abs() < f64::EPSILON
                && (a.tests_pct - split.tests_pct).abs() < f64::EPSILON
        })
    }

    /// Classify a query into a task type using keyword heuristics.
    pub fn classify_task(query: &str) -> TaskType {
        let lower = query.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();

        let bug_keywords = ["fix", "bug", "error", "fail"];
        let feature_keywords = ["add", "implement", "create", "new"];
        let refactor_keywords = ["refactor", "extract", "move", "rename"];
        let understand_keywords = ["find", "where", "how", "what", "explain"];

        // Score each category; first match wins (order: most specific first).
        for word in &words {
            if bug_keywords.iter().any(|k| word.contains(k)) {
                return TaskType::BugFix;
            }
        }
        for word in &words {
            if refactor_keywords.iter().any(|k| word.contains(k)) {
                return TaskType::Refactor;
            }
        }
        for word in &words {
            if feature_keywords.iter().any(|k| word.contains(k)) {
                return TaskType::FeatureAdd;
            }
        }
        for word in &words {
            if understand_keywords.iter().any(|k| word.contains(k)) {
                return TaskType::Understand;
            }
        }

        TaskType::Unknown
    }

    /// Save bandit state to JSON at the given path.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        std::fs::write(path, json)
    }

    /// Load bandit state from a JSON file.
    pub fn load(path: &str) -> std::io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })
    }
}

impl Default for BudgetBandit {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bandit_explores_all_arms_first() {
        let bandit = BudgetBandit::new();

        // First call should return arm 0 (first unpulled).
        let split = bandit.select(TaskType::BugFix);
        assert!((split.code_pct - 0.60).abs() < f64::EPSILON);
    }

    #[test]
    fn update_and_select_favors_rewarded_arm() {
        let mut bandit = BudgetBandit::new();

        // Pull all arms equally with low reward so exploration is exhausted.
        for _ in 0..10 {
            for i in 0..4 {
                bandit.update(TaskType::BugFix, i, 0.1);
            }
        }

        // Give arm 3 (test-heavy) many high rewards to dominate.
        for _ in 0..30 {
            bandit.update(TaskType::BugFix, 3, 1.0);
        }

        let split = bandit.select(TaskType::BugFix);
        // Arm 3 should be favored due to much higher average reward.
        assert!((split.tests_pct - 0.35).abs() < f64::EPSILON);
    }

    #[test]
    fn classify_task_keywords() {
        assert_eq!(BudgetBandit::classify_task("fix the login bug"), TaskType::BugFix);
        assert_eq!(BudgetBandit::classify_task("add new endpoint"), TaskType::FeatureAdd);
        assert_eq!(BudgetBandit::classify_task("refactor user module"), TaskType::Refactor);
        assert_eq!(BudgetBandit::classify_task("where is the config"), TaskType::Understand);
        assert_eq!(BudgetBandit::classify_task("deploy to staging"), TaskType::Unknown);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut bandit = BudgetBandit::new();
        bandit.update(TaskType::FeatureAdd, 1, 0.8);
        bandit.update(TaskType::FeatureAdd, 1, 0.9);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bandit.json");
        let path_str = path.to_str().unwrap();

        bandit.save(path_str).unwrap();
        let loaded = BudgetBandit::load(path_str).unwrap();

        let stats = loaded.arms.get(&TaskType::FeatureAdd).unwrap();
        assert_eq!(stats[1].1, 2); // pull count
        assert!((stats[1].0 - 1.7).abs() < 1e-9); // total reward 0.8 + 0.9
    }

    #[test]
    fn arm_index_for_returns_correct_index() {
        assert_eq!(BudgetBandit::arm_index_for(&ARMS[0]), Some(0));
        assert_eq!(BudgetBandit::arm_index_for(&ARMS[3]), Some(3));

        let custom = BudgetSplit {
            code_pct: 0.50,
            signature_pct: 0.50,
            map_pct: 0.0,
            tests_pct: 0.0,
        };
        assert_eq!(BudgetBandit::arm_index_for(&custom), None);
    }
}
