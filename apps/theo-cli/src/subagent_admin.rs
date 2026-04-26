//! Admin CLI commands for sub-agent system: `subagent`, `checkpoints`, `agents`.
//!
//! Track A/C/D — fecha DODs do plano (Fases 9, 10) com superficie CLI.
//!
//! Commands:
//! - `theo subagent list` — list persisted runs
//! - `theo subagent status <id>` — show details for a run
//! - `theo subagent abandon <id>` — mark non-terminal run as abandoned
//! - `theo subagent cleanup [--days N]` — remove old terminal runs
//! - `theo checkpoints list` — list checkpoints for current workdir
//! - `theo checkpoints restore <commit>` — restore workdir to a checkpoint
//! - `theo checkpoints cleanup [--days N]` — prune old checkpoints
//! - `theo agents approve` — interactive approval of pending project agents

use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;

// T3.3 / find_p3_009 / ADR-023 — switch to the
// `theo_application::cli_runtime` re-export façade so the
// apps/* → theo-application layer rule is honoured.
use theo_application::cli_runtime::approval::{
    compute_current_manifest, load_approved, persist_approved, ApprovalManifest, ApprovedEntry,
    sha256_hex,
};
use theo_application::cli_runtime::{
    AgentConfig, CheckpointManager, EventBus, FileSubagentRunStore, Resumer, RunStatus,
    SubAgentManager, SubAgentRegistry,
};

#[derive(Subcommand)]
pub enum SubagentCmd {
    /// List persisted sub-agent runs (newest first).
    List,
    /// Show details for a specific run.
    Status {
        /// Run ID.
        run_id: String,
    },
    /// Mark a non-terminal run as abandoned (Archon principle: user-driven only).
    Abandon {
        /// Run ID.
        run_id: String,
    },
    /// Remove old terminal runs.
    Cleanup {
        /// Maximum age in days (default: 7).
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
    /// Resume a non-terminal sub-agent run. Reconstructs history from the
    /// event log and re-spawns the agent. Idempotent: terminal runs
    /// (Completed/Failed/Cancelled/Abandoned) are rejected.
    Resume {
        /// Run ID.
        run_id: String,
        /// Optional new objective (overrides the original).
        #[arg(long)]
        objective: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CheckpointsCmd {
    /// List checkpoints for the current workdir.
    List,
    /// Restore the workdir to a previous checkpoint.
    Restore {
        /// Commit SHA (4-64 hex chars).
        commit: String,
    },
    /// Remove checkpoints older than N days (default: 30).
    Cleanup {
        #[arg(long, default_value_t = 30)]
        days: u32,
    },
}

#[derive(Subcommand)]
pub enum AgentsCmd {
    /// Show pending and approved project agents (S3 manifest).
    Status,
    /// Interactively approve all pending project agents.
    Approve {
        /// Skip prompt — approve all pending without confirmation.
        #[arg(long)]
        all: bool,
    },
    /// Revoke all approvals (deletes .theo/.agents-approved).
    Revoke,
}

pub fn handle_subagent(cmd: SubagentCmd, project_dir: &std::path::Path) -> anyhow::Result<()> {
    let store = FileSubagentRunStore::new(runs_base_dir(project_dir));
    match cmd {
        SubagentCmd::List => {
            let ids = store.list().map_err(|e| anyhow::anyhow!("{}", e))?;
            if ids.is_empty() {
                println!("No persisted sub-agent runs.");
                return Ok(());
            }
            println!("{:<32} {:<14} {:<12} {:>8} {:>8}", "RUN_ID", "AGENT", "STATUS", "ITER", "TOKENS");
            for id in ids {
                if let Ok(run) = store.load(&id) {
                    println!(
                        "{:<32} {:<14} {:<12} {:>8} {:>8}",
                        truncate(&run.run_id, 32),
                        truncate(&run.agent_name, 14),
                        format!("{:?}", run.status).to_lowercase(),
                        run.iterations_used,
                        run.tokens_used,
                    );
                }
            }
        }
        SubagentCmd::Status { run_id } => {
            let run = store
                .load(&run_id)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("Run: {}", run.run_id);
            println!("Agent: {} ({})", run.agent_name, run.agent_source);
            println!("Status: {:?}", run.status);
            println!("Objective: {}", run.objective);
            println!("Started: {}", run.started_at);
            if let Some(f) = run.finished_at {
                println!("Finished: {}", f);
            }
            println!("Iterations: {}", run.iterations_used);
            println!("Tokens: {}", run.tokens_used);
            if let Some(s) = &run.summary {
                println!("\nSummary:\n{}", s);
            }
            if let Some(structured) = &run.structured_output {
                println!("\nStructured output:");
                println!("{}", serde_json::to_string_pretty(structured)?);
            }
            let events = store
                .list_events(&run_id)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            if !events.is_empty() {
                println!("\nEvents ({}):", events.len());
                for e in events.iter().take(20) {
                    println!("  [{}] {}", e.timestamp, e.event_type);
                }
            }
        }
        SubagentCmd::Abandon { run_id } => {
            let run = store
                .abandon(&run_id)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            if matches!(run.status, RunStatus::Abandoned) {
                println!("✓ Run '{}' marked as abandoned.", run_id);
            } else {
                println!(
                    "Run '{}' was already terminal ({:?}). No change.",
                    run_id, run.status
                );
            }
        }
        SubagentCmd::Cleanup { days } => {
            let max_age_seconds = (days as i64) * 86_400;
            let removed = store
                .cleanup(max_age_seconds)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("✓ Removed {} terminal run(s) older than {} day(s).", removed, days);
        }
        SubagentCmd::Resume { run_id, objective } => {
            return handle_resume(&store, project_dir, &run_id, objective.as_deref());
        }
    }
    Ok(())
}

/// Phase 16 — Resume entry point.
///
/// Loads the run, validates that it is non-terminal, builds a SubAgentManager
/// with the default registry, and re-spawns the agent with the reconstructed
/// history. Idempotent: terminal runs are rejected with a clear error.
///
/// LLM configuration uses the project's default chain (env, OAuth, providers
/// registry). The resume CLI does not currently expose `--provider`/`--model`
/// flags — the resumed run inherits the spec's `model_override` if any.
pub fn handle_resume(
    store: &FileSubagentRunStore,
    project_dir: &std::path::Path,
    run_id: &str,
    objective_override: Option<&str>,
) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
    rt.block_on(async {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_registry(
            AgentConfig::default(),
            bus,
            project_dir.to_path_buf(),
            Arc::new(SubAgentRegistry::with_builtins()),
        );
        let resumer = Resumer::new(store, &manager);
        match resumer.resume_with_objective(run_id, objective_override).await {
            Ok(result) => {
                println!("✓ Resume completed for '{}'.", run_id);
                println!("  Success: {}", result.success);
                if !result.summary.is_empty() {
                    println!("  Summary: {}", truncate(&result.summary, 200));
                }
                if result.iterations_used > 0 {
                    println!("  Iterations used: {}", result.iterations_used);
                }
                if result.tokens_used > 0 {
                    println!("  Tokens used: {}", result.tokens_used);
                }
                Ok(())
            }
            // Plan §16 line 596: NotResumable is a normal user condition, not
            // an error — print guidance instead of bubbling up exit-code 1.
            Err(theo_application::cli_runtime::ResumeError::NotResumable {
                status, ..
            }) => {
                println!(
                    "Run '{}' is in terminal status '{}'. \
                     Use `theo subagent abandon {}` to mark it as abandoned, \
                     then re-spawn fresh.",
                    run_id, status, run_id
                );
                Ok(())
            }
            Err(theo_application::cli_runtime::ResumeError::NotFound(_)) => {
                println!(
                    "Run '{}' not found in {}. \
                     Use `theo subagent list` to see available runs.",
                    run_id,
                    project_dir.display()
                );
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("resume failed: {}", e)),
        }
    })
}

pub fn handle_checkpoints(
    cmd: CheckpointsCmd,
    workdir: &std::path::Path,
) -> anyhow::Result<()> {
    let base = checkpoints_base_dir();
    let manager = CheckpointManager::new(workdir, &base)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    match cmd {
        CheckpointsCmd::List => {
            let list = manager.list().map_err(|e| anyhow::anyhow!("{}", e))?;
            if list.is_empty() {
                println!("No checkpoints for {}.", workdir.display());
                return Ok(());
            }
            println!("{:<42} {:<10} LABEL", "COMMIT", "TIME");
            for cp in list {
                println!(
                    "{:<42} {:<10} {}",
                    cp.commit,
                    cp.timestamp_unix,
                    cp.label
                );
            }
        }
        CheckpointsCmd::Restore { commit } => {
            manager
                .restore(&commit)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("✓ Workdir restored to {}.", commit);
        }
        CheckpointsCmd::Cleanup { days } => {
            let max_age_seconds = (days as i64) * 86_400;
            let pruned = manager
                .cleanup(max_age_seconds)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("✓ Pruned {} checkpoint(s) older than {} day(s).", pruned, days);
        }
    }
    Ok(())
}

pub fn handle_agents(cmd: AgentsCmd, project_dir: &std::path::Path) -> anyhow::Result<()> {
    let agents_dir = project_dir.join(".theo").join("agents");
    match cmd {
        AgentsCmd::Status => {
            if !agents_dir.exists() {
                println!("No project agents directory at {}.", agents_dir.display());
                return Ok(());
            }
            let current = compute_current_manifest(&agents_dir)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let approved = load_approved(project_dir).map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("Project agents directory: {}", agents_dir.display());
            println!("Files found: {}", current.len());
            println!("Approved: {}", approved.approved.len());
            println!();
            for entry in &current {
                let status = if approved.is_approved(&entry.file, &entry.sha256) {
                    "✓ approved"
                } else if approved
                    .approved
                    .iter()
                    .any(|a| a.file == entry.file)
                {
                    "⚠ modified (re-approval needed)"
                } else {
                    "○ pending"
                };
                println!("  {:<20} {}", entry.file, status);
            }
        }
        AgentsCmd::Approve { all } => {
            if !agents_dir.exists() {
                println!("No agents directory at {}; nothing to approve.", agents_dir.display());
                return Ok(());
            }
            let current = compute_current_manifest(&agents_dir)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let approved = load_approved(project_dir).map_err(|e| anyhow::anyhow!("{}", e))?;
            let pending: Vec<&ApprovedEntry> = current
                .iter()
                .filter(|c| !approved.is_approved(&c.file, &c.sha256))
                .collect();

            if pending.is_empty() {
                println!("✓ No pending project agents.");
                return Ok(());
            }

            println!("Pending project agents:");
            for p in &pending {
                println!("  - {} (sha {}…)", p.file, &p.sha256[..12]);
            }

            let confirm = if all {
                true
            } else {
                print!("\nApprove all? (y/N): ");
                use std::io::Write;
                std::io::stdout().flush().ok();
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok();
                input.trim().eq_ignore_ascii_case("y")
            };

            if !confirm {
                println!("Aborted. No changes made.");
                return Ok(());
            }

            // Build new manifest: keep approved-and-still-current + add pending
            let mut entries: Vec<ApprovedEntry> = Vec::new();
            for c in &current {
                entries.push(ApprovedEntry {
                    file: c.file.clone(),
                    sha256: c.sha256.clone(),
                });
            }
            // Sort for determinism
            entries.sort_by(|a, b| a.file.cmp(&b.file));
            let manifest = ApprovalManifest { approved: entries };
            persist_approved(project_dir, &manifest).map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("✓ Approved {} agent(s).", pending.len());
        }
        AgentsCmd::Revoke => {
            let path = project_dir.join(".theo").join(".agents-approved");
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("✓ Removed approval manifest at {}.", path.display());
            } else {
                println!("No approval manifest to revoke.");
            }
        }
    }
    Ok(())
}

// Helpers ---------------------------------------------------------------------

fn runs_base_dir(project_dir: &std::path::Path) -> PathBuf {
    project_dir.join(".theo").join("subagent")
}

fn checkpoints_base_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".theo").join("checkpoints")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
}

/// Helper for tests to silence the `sha256_hex` unused-import warning when
/// only some sub-modules pull it.
#[allow(dead_code)]
fn _import_keep() {
    let _ = sha256_hex;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn handle_subagent_list_empty_returns_ok() {
        let dir = TempDir::new().unwrap();
        let res = handle_subagent(SubagentCmd::List, dir.path());
        assert!(res.is_ok());
    }

    #[test]
    fn handle_subagent_status_unknown_returns_err() {
        let dir = TempDir::new().unwrap();
        let res = handle_subagent(
            SubagentCmd::Status {
                run_id: "missing".into(),
            },
            dir.path(),
        );
        assert!(res.is_err());
    }

    #[test]
    fn handle_agents_status_no_dir_returns_ok() {
        let dir = TempDir::new().unwrap();
        let res = handle_agents(AgentsCmd::Status, dir.path());
        assert!(res.is_ok());
    }

    #[test]
    fn handle_agents_approve_no_dir_returns_ok() {
        let dir = TempDir::new().unwrap();
        let res = handle_agents(AgentsCmd::Approve { all: true }, dir.path());
        assert!(res.is_ok());
    }

    #[test]
    fn handle_agents_revoke_no_manifest_returns_ok() {
        let dir = TempDir::new().unwrap();
        let res = handle_agents(AgentsCmd::Revoke, dir.path());
        assert!(res.is_ok());
    }

    #[test]
    fn handle_agents_approve_all_creates_manifest() {
        let dir = TempDir::new().unwrap();
        let agents = dir.path().join(".theo").join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("custom.md"),
            "---\ndescription: test\n---\nbody",
        )
        .unwrap();

        let res = handle_agents(AgentsCmd::Approve { all: true }, dir.path());
        assert!(res.is_ok());

        let manifest_path = dir.path().join(".theo").join(".agents-approved");
        assert!(manifest_path.exists());
        let approved = load_approved(dir.path()).unwrap();
        assert_eq!(approved.approved.len(), 1);
        assert_eq!(approved.approved[0].file, "custom.md");
    }
}


// ---------------------------------------------------------------------------
// Phase 16 (sota-gaps): resume CLI tests in a top-level submodule so
// `cargo test -p theo --bin theo subagent_admin::resume` (the plan’s
// literal verify command) targets exactly this surface.
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod resume {
    use super::{handle_subagent, SubagentCmd};
    use tempfile::TempDir;

    /// Plan §16 line 596: NotFound is a normal user condition (typo'd run id),
    /// not a CLI error — guidance is printed and exit code 0 is returned.
    #[test]
    fn handle_subagent_resume_unknown_returns_ok_with_guidance() {
        let dir = TempDir::new().unwrap();
        let res = handle_subagent(
            SubagentCmd::Resume {
                run_id: "missing".into(),
                objective: None,
            },
            dir.path(),
        );
        assert!(
            res.is_ok(),
            "missing run id is friendly UX (Ok with println), not Err: {:?}",
            res
        );
    }

    /// Plan §16 line 596: terminal status prints a tip (use `abandon`), not
    /// an error. Exit code 0.
    #[test]
    fn handle_subagent_resume_terminal_run_returns_ok_with_guidance() {
        // T3.3 — re-route via theo-application; SubagentRun stays direct
        // (not in cli_runtime re-exports yet — follow-up).
        use theo_application::cli_runtime::{FileSubagentRunStore, RunStatus, SubagentRun};
        use theo_domain::agent_spec::AgentSpec;
        let dir = TempDir::new().unwrap();
        let store_dir = dir.path().join(".theo").join("subagent");
        let store = FileSubagentRunStore::new(&store_dir);
        let spec = AgentSpec::on_demand("x", "y");
        let mut run = SubagentRun::new_running(
            "r-term", None, &spec, "y", "/tmp", None,
        );
        run.status = RunStatus::Completed;
        store.save(&run).unwrap();

        let res = handle_subagent(
            SubagentCmd::Resume {
                run_id: "r-term".into(),
                objective: None,
            },
            dir.path(),
        );
        assert!(
            res.is_ok(),
            "terminal run resume is friendly UX (Ok with abandon hint): {:?}",
            res
        );
    }

    /// Idempotency: invoking resume on the same terminal run twice produces
    /// identical UX both times.
    #[test]
    fn handle_subagent_resume_terminal_run_is_idempotent() {
        // T3.3 — re-route via theo-application; SubagentRun stays direct
        // (not in cli_runtime re-exports yet — follow-up).
        use theo_application::cli_runtime::{FileSubagentRunStore, RunStatus, SubagentRun};
        use theo_domain::agent_spec::AgentSpec;
        let dir = TempDir::new().unwrap();
        let store_dir = dir.path().join(".theo").join("subagent");
        let store = FileSubagentRunStore::new(&store_dir);
        let spec = AgentSpec::on_demand("x", "y");
        let mut run = SubagentRun::new_running(
            "r-done", None, &spec, "y", "/tmp", None,
        );
        run.status = RunStatus::Completed;
        store.save(&run).unwrap();

        for _ in 0..3 {
            let res = handle_subagent(
                SubagentCmd::Resume {
                    run_id: "r-done".into(),
                    objective: None,
                },
                dir.path(),
            );
            assert!(res.is_ok(), "every call must be Ok (idempotent)");
        }
    }
}
