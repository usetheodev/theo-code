//! Routing-benchmark harness (plan phase R0).
//!
//! Loads labelled fixtures from `.theo/fixtures/routing/*.json` and replays
//! them against a pluggable cost model so `NullRouter` vs `RuleBasedRouter`
//! (future phases) can be compared on the same dataset.
//!
//! This is intentionally simulated — real LLM calls are out of scope for the
//! offline-first benchmark. The cost model uses the fixture's
//! `expected_token_budget` × a per-tier price-per-token to derive
//! `avg_cost_per_task`. `task_success_rate` uses a heuristic: a choice
//! succeeds iff its tier is >= the difficulty label (simple < medium < complex).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// One labelled case from `.theo/fixtures/routing/*.json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoutingCase {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub expected_token_budget: u32,
}

/// Minimal cost/tier model exposed to the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Cheap,
    Default,
    Strong,
}

impl Tier {
    /// Simulated $ cost per 1k tokens at each tier.
    pub fn price_per_1k_tokens(self) -> f64 {
        match self {
            Tier::Cheap => 0.0008,
            Tier::Default => 0.003,
            Tier::Strong => 0.015,
        }
    }

    /// Mapping of difficulty label -> minimum tier that solves it.
    /// Used by the success heuristic.
    pub fn required_for(label: &str) -> Option<Self> {
        match label {
            "simple" => Some(Tier::Cheap),
            "medium" => Some(Tier::Default),
            "complex" => Some(Tier::Strong),
            _ => None,
        }
    }
}

/// Aggregated report serialised to stdout as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingReport {
    pub cases_total: usize,
    pub cases_passed: usize,
    pub avg_cost_per_task: f64,
    pub task_success_rate: f64,
    pub p50_turn_latency_us: u64,
    pub per_label: Vec<LabelBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelBreakdown {
    pub label: String,
    pub count: usize,
    pub avg_cost: f64,
    pub success_rate: f64,
}

/// Failure mode for harness setup (not the case evaluation itself).
#[derive(Debug, thiserror::Error)]
pub enum MetricsError {
    #[error("failed to read fixture directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse fixture {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("fixture directory {0} is empty")]
    Empty(PathBuf),
}

/// Load every `*.json` case under `dir`.
pub fn load_cases(dir: &Path) -> Result<Vec<RoutingCase>, MetricsError> {
    let entries = fs::read_dir(dir).map_err(|source| MetricsError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut cases = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|source| MetricsError::ReadDir {
            path: path.clone(),
            source,
        })?;
        let case: RoutingCase =
            serde_json::from_str(&raw).map_err(|source| MetricsError::Parse {
                path: path.clone(),
                source,
            })?;
        cases.push(case);
    }
    if cases.is_empty() {
        return Err(MetricsError::Empty(dir.to_path_buf()));
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(cases)
}

/// Replay `cases` using `pick_tier` as the router stand-in.
/// Each case is scored: success iff the picked tier >= required tier for
/// its label; cost is tokens × tier price.
pub fn run_cases<F>(cases: &[RoutingCase], mut pick_tier: F) -> RoutingReport
where
    F: FnMut(&RoutingCase) -> Tier,
{
    let mut total_cost = 0.0f64;
    let mut passed = 0usize;
    let mut latencies_us = Vec::with_capacity(cases.len());
    let mut by_label: std::collections::BTreeMap<String, (usize, f64, usize)> =
        std::collections::BTreeMap::new();

    for case in cases {
        let start = Instant::now();
        let tier = pick_tier(case);
        let elapsed = start.elapsed().as_micros() as u64;
        latencies_us.push(elapsed);

        let tokens = case.expected_token_budget as f64;
        let cost = (tokens / 1000.0) * tier.price_per_1k_tokens();
        total_cost += cost;

        let success = Tier::required_for(&case.label)
            .map(|req| tier >= req)
            .unwrap_or(false);
        if success {
            passed += 1;
        }

        let entry = by_label
            .entry(case.label.clone())
            .or_insert((0, 0.0, 0));
        entry.0 += 1;
        entry.1 += cost;
        if success {
            entry.2 += 1;
        }
    }

    latencies_us.sort_unstable();
    let p50 = latencies_us
        .get(latencies_us.len() / 2)
        .copied()
        .unwrap_or(0);

    let per_label = by_label
        .into_iter()
        .map(|(label, (count, cost_sum, succ))| LabelBreakdown {
            label,
            count,
            avg_cost: cost_sum / count as f64,
            success_rate: succ as f64 / count as f64,
        })
        .collect();

    RoutingReport {
        cases_total: cases.len(),
        cases_passed: passed,
        avg_cost_per_task: total_cost / cases.len() as f64,
        task_success_rate: passed as f64 / cases.len() as f64,
        p50_turn_latency_us: p50,
        per_label,
    }
}
