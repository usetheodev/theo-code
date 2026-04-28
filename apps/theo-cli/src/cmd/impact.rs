//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_impact(repo_path: &Path, file_path: &str) {
    let mut pipeline = Pipeline::with_defaults();
    let (files, _) = theo_application::use_cases::extraction::extract_repo(repo_path);
    pipeline.build_graph(&files);
    let _ = pipeline.add_git_cochanges(repo_path);
    pipeline.cluster();

    let report = pipeline.impact_analysis(file_path);

    println!("=== Impact Analysis ===");
    println!();
    println!("File: {}", report.edited_file);
    println!("BFS depth: {}", report.bfs_depth);
    println!();
    println!(
        "Affected communities ({}):",
        report.affected_communities.len()
    );
    for c in &report.affected_communities {
        println!("  - {}", c);
    }
    println!();
    println!(
        "Tests covering edit ({}):",
        report.tests_covering_edit.len()
    );
    for t in &report.tests_covering_edit {
        println!("  - {}", t);
    }
    println!();
    println!(
        "Co-change candidates ({}):",
        report.co_change_candidates.len()
    );
    for c in &report.co_change_candidates {
        println!("  - {}", c);
    }
    println!();
    println!("Risk alerts ({}):", report.risk_alerts.len());
    for a in &report.risk_alerts {
        println!("  ⚠ {}", a);
    }
}

