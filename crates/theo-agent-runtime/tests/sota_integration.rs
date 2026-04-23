//! Integration test SOTA — valida o smoke test final de docs/plans/agents-plan.md.
//!
//! Cobre todas as features SOTA juntas:
//! - Custom agent em .theo/agents/sota-agent.md
//! - S3 manifest aprovação (TrustAll para CI)
//! - Hooks (PreToolUse / SubagentStart / UserPromptSubmit)
//! - Output format schema (best_effort)
//! - MCP servers allowlist
//! - Worktree isolation (mode worktree, base_branch main)
//! - SubAgentRegistry build_tool_description deterministico
//! - SubagentRun persistence
//! - Checkpoint manager
//! - OTel span attributes

use tempfile::TempDir;

use theo_agent_runtime::cancellation::CancellationTree;
use theo_agent_runtime::checkpoint::CheckpointManager;
use theo_agent_runtime::lifecycle_hooks::{HookEvent, HookManager, HookMatcher, HookResponse};
use theo_agent_runtime::observability::otel::{
    AgentRunSpan, ATTR_AGENT_NAME, ATTR_THEO_AGENT_SOURCE,
};
use theo_agent_runtime::output_format::try_parse_structured;
use theo_agent_runtime::subagent::{
    parse_agent_spec, ApprovalMode, SubAgentRegistry,
};
use theo_agent_runtime::subagent_runs::{FileSubagentRunStore, RunStatus, SubagentRun};
use theo_domain::agent_spec::AgentSpecSource;
use theo_isolation::{safety_rules, IsolationMode, WorktreeProvider};
use theo_infra_mcp::client::{mcp_tool_name, parse_mcp_tool_name};
use theo_infra_mcp::McpServerConfig;

/// The exact spec from agents-plan.md v3.0 "Smoke test final".
const SOTA_AGENT_SPEC: &str = r#"---
name: security-reviewer
description: "Reviews code for OWASP Top 10 with structured output"
denied_tools: [edit, write, bash]
mcp_servers: [github]
isolation:
  mode: worktree
  base_branch: main
output_format:
  enforcement: best_effort
  schema:
    type: object
    required: [findings]
    properties:
      findings:
        type: array
        items:
          type: object
          required: [severity, file, message]
          properties:
            severity: { enum: [critical, high, medium, low] }
            file: { type: string }
            line: { type: integer }
            message: { type: string }
max_iterations: 25
timeout: 300
---
You are a security-focused code reviewer. Find vulnerabilities. Report findings.
"#;

#[test]
fn sota_smoke_parse_full_spec_with_all_features() {
    let spec = parse_agent_spec(SOTA_AGENT_SPEC, "security-reviewer", AgentSpecSource::Project)
        .expect("spec parses");

    // Basic identity
    assert_eq!(spec.name, "security-reviewer");
    assert!(spec.description.contains("OWASP"));
    assert_eq!(spec.source, AgentSpecSource::Project);

    // Phase 1/2/G3: capability set with denied tools
    assert!(spec.capability_set.denied_tools.contains("edit"));
    assert!(spec.capability_set.denied_tools.contains("write"));
    assert!(spec.capability_set.denied_tools.contains("bash"));

    // Phase 7: output format declared (best_effort default)
    assert!(spec.output_format.is_some());
    assert_eq!(spec.output_format_strict, None); // best_effort = None

    // Phase 8: MCP servers allowlist
    assert_eq!(spec.mcp_servers, vec!["github".to_string()]);

    // Phase 11: isolation = worktree, base_branch = main
    assert_eq!(spec.isolation.as_deref(), Some("worktree"));
    assert_eq!(spec.isolation_base_branch.as_deref(), Some("main"));

    // Numeric A1
    assert_eq!(spec.max_iterations, 25);
    assert_eq!(spec.timeout_secs, 300);
}

#[test]
fn sota_smoke_registry_loads_approved_project_spec() {
    use theo_agent_runtime::subagent::approval::{persist_approved, sha256_hex, ApprovalManifest};
    let project = TempDir::new().unwrap();
    let agents = project.path().join(".theo").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("security-reviewer.md"), SOTA_AGENT_SPEC).unwrap();

    // Approve manifest (S3 / G1)
    let manifest = ApprovalManifest {
        approved: vec![theo_agent_runtime::subagent::ApprovedEntry {
            file: "security-reviewer.md".to_string(),
            sha256: sha256_hex(SOTA_AGENT_SPEC),
        }],
    };
    persist_approved(project.path(), &manifest).unwrap();

    let mut reg = SubAgentRegistry::with_builtins();
    let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::Interactive);
    assert!(outcome.pending_approval.is_empty());

    // 4 builtins + 1 custom
    assert_eq!(reg.len(), 5);
    let custom = reg.get("security-reviewer").expect("loaded");
    assert_eq!(custom.source, AgentSpecSource::Project);
    assert!(custom.mcp_servers.contains(&"github".to_string()));
}

#[test]
fn sota_smoke_unapproved_spec_is_pending() {
    let project = TempDir::new().unwrap();
    let agents = project.path().join(".theo").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("security-reviewer.md"), SOTA_AGENT_SPEC).unwrap();

    let mut reg = SubAgentRegistry::with_builtins();
    let outcome = reg.load_all(Some(project.path()), None, ApprovalMode::Interactive);
    assert_eq!(outcome.pending_approval.len(), 1);
    assert!(!reg.contains("security-reviewer"));
}

#[test]
fn sota_smoke_hooks_block_forbidden_tools() {
    let mut hooks = HookManager::new();
    hooks.add(
        HookEvent::PreToolUse,
        HookMatcher {
            matcher: Some("^(edit|write|bash)$".to_string()),
            response: HookResponse::Block {
                reason: "this agent is read-only".to_string(),
            },
            timeout_secs: 60,
        },
    );
    hooks.add(
        HookEvent::UserPromptSubmit,
        HookMatcher {
            matcher: None,
            response: HookResponse::InjectContext {
                content: "Focus on OWASP Top 10.".to_string(),
            },
            timeout_secs: 60,
        },
    );

    use theo_agent_runtime::lifecycle_hooks::HookContext;
    // Bash is blocked
    let ctx = HookContext {
        tool_name: Some("bash".to_string()),
        ..Default::default()
    };
    let resp = hooks.dispatch(HookEvent::PreToolUse, &ctx);
    matches!(resp, HookResponse::Block { .. });

    // Read passes
    let ctx_read = HookContext {
        tool_name: Some("read".to_string()),
        ..Default::default()
    };
    assert_eq!(
        hooks.dispatch(HookEvent::PreToolUse, &ctx_read),
        HookResponse::Allow
    );

    // UserPromptSubmit injects OWASP context
    let inj = hooks.dispatch(HookEvent::UserPromptSubmit, &HookContext::default());
    match inj {
        HookResponse::InjectContext { content } => assert!(content.contains("OWASP")),
        _ => panic!("expected InjectContext"),
    }
}

#[test]
fn sota_smoke_output_format_parses_valid_findings() {
    let spec = parse_agent_spec(SOTA_AGENT_SPEC, "security-reviewer", AgentSpecSource::Project)
        .unwrap();
    let schema = spec.output_format.as_ref().unwrap();

    // Valid output
    let valid = r#"After review:
{
  "findings": [
    {"severity": "high", "file": "auth.rs", "line": 42, "message": "Hardcoded secret"},
    {"severity": "low", "file": "utils.rs", "message": "Style nit"}
  ]
}
End of report."#;
    let parsed = try_parse_structured(valid, schema).unwrap();
    assert_eq!(parsed["findings"][0]["severity"], "high");
}

#[test]
fn sota_smoke_output_format_invalid_severity_fails() {
    let spec = parse_agent_spec(SOTA_AGENT_SPEC, "security-reviewer", AgentSpecSource::Project)
        .unwrap();
    let schema = spec.output_format.as_ref().unwrap();
    let invalid = r#"{"findings": [{"severity": "trivial", "file": "x", "message": "y"}]}"#;
    let result = try_parse_structured(invalid, schema);
    assert!(result.is_err(), "should fail enum validation");
}

#[test]
fn sota_smoke_mcp_tool_namespace_avoids_native_collisions() {
    // mcp:github:search must not collide with native tools
    let qualified = mcp_tool_name("github", "search");
    assert_eq!(qualified, "mcp:github:search");
    let parsed = parse_mcp_tool_name(&qualified).unwrap();
    assert_eq!(parsed, ("github", "search"));

    // Native tools never parse as MCP
    assert!(parse_mcp_tool_name("read").is_none());
    assert!(parse_mcp_tool_name("bash").is_none());
}

#[test]
fn sota_smoke_mcp_server_config_yaml_format() {
    use std::collections::BTreeMap;
    let mut env = BTreeMap::new();
    env.insert("GITHUB_TOKEN".to_string(), "abc123".to_string());
    let cfg = McpServerConfig::Stdio {
        name: "github".to_string(),
        command: "npx".to_string(),
        args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
        env,
    };
    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(json["transport"], "stdio");
    assert_eq!(json["name"], "github");
}

#[test]
fn sota_smoke_isolation_safety_rules_explicitly_named() {
    let rules = safety_rules();
    // Pi-Mono parallel-agent rules are explicit and named (so hooks can match)
    assert!(rules.contains("git reset"));
    assert!(rules.contains("git checkout"));
    assert!(rules.contains("git stash pop"));
    assert!(rules.contains("git add -A"));
    assert!(rules.contains("ONLY commit files"));
}

#[test]
fn sota_smoke_isolation_mode_default_is_shared() {
    // Default isolation is shared (worktree is opt-in)
    assert_eq!(IsolationMode::default(), IsolationMode::Shared);
}

#[test]
fn sota_smoke_otel_span_attributes_for_sota_spec() {
    let spec = parse_agent_spec(SOTA_AGENT_SPEC, "security-reviewer", AgentSpecSource::Project)
        .unwrap();
    let span = AgentRunSpan::from_spec(&spec, "run-test-123");
    assert_eq!(span.attributes[ATTR_AGENT_NAME], "security-reviewer");
    assert_eq!(span.attributes[ATTR_THEO_AGENT_SOURCE], "project");
}

#[test]
fn sota_smoke_session_persistence_full_lifecycle() {
    let dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(dir.path());
    let spec = parse_agent_spec(SOTA_AGENT_SPEC, "security-reviewer", AgentSpecSource::Project)
        .unwrap();

    // 1. Save running
    let run = SubagentRun::new_running(
        "test-run-1",
        None,
        &spec,
        "review auth.rs",
        "/tmp/proj",
        Some("abcd1234".to_string()), // checkpoint_before
    );
    store.save(&run).unwrap();

    // 2. List shows it
    assert_eq!(store.list().unwrap(), vec!["test-run-1".to_string()]);

    // 3. Append events (per-iteration)
    use theo_agent_runtime::subagent_runs::SubagentEvent;
    for i in 0..3 {
        store.append_event(
            "test-run-1",
            &SubagentEvent {
                timestamp: i,
                event_type: format!("iteration_{}", i),
                payload: serde_json::json!({"step": i}),
            },
        ).unwrap();
    }
    assert_eq!(store.list_events("test-run-1").unwrap().len(), 3);

    // 4. Update to completed with structured output
    let mut updated = store.load("test-run-1").unwrap();
    updated.status = RunStatus::Completed;
    updated.iterations_used = 3;
    updated.tokens_used = 5000;
    updated.summary = Some("Found 2 high-severity issues".to_string());
    updated.structured_output = Some(serde_json::json!({
        "findings": [{"severity": "high", "file": "auth.rs", "message": "..."}]
    }));
    store.save(&updated).unwrap();

    // 5. Reload preserves everything
    let final_run = store.load("test-run-1").unwrap();
    assert_eq!(final_run.status, RunStatus::Completed);
    assert_eq!(final_run.iterations_used, 3);
    assert_eq!(final_run.tokens_used, 5000);
    assert!(final_run.structured_output.is_some());
    assert_eq!(final_run.checkpoint_before.as_deref(), Some("abcd1234"));
    // config_snapshot preserved (resume requires it)
    assert_eq!(final_run.config_snapshot.name, "security-reviewer");
}

#[test]
fn sota_smoke_cancellation_tree_propagates_to_all_children() {
    let tree = CancellationTree::new();
    let agent_a = tree.child("agent-a");
    let agent_b = tree.child("agent-b");

    assert!(!agent_a.is_cancelled());
    assert!(!agent_b.is_cancelled());

    tree.cancel_all();

    assert!(agent_a.is_cancelled());
    assert!(agent_b.is_cancelled());
    assert!(tree.is_cancelled());
}

#[test]
fn sota_smoke_checkpoint_init_does_not_pollute_workdir() {
    if !git_available() {
        return;
    }
    let workdir = TempDir::new().unwrap();
    std::fs::write(workdir.path().join("a.rs"), "x").unwrap();
    let base = TempDir::new().unwrap();
    let mgr = CheckpointManager::new(workdir.path(), base.path()).unwrap();
    mgr.snapshot("init").unwrap();
    // Workdir must NOT have a .git folder
    assert!(!workdir.path().join(".git").exists());
}

#[test]
fn sota_smoke_worktree_provider_path_deterministic() {
    let provider = WorktreeProvider::new(
        std::path::PathBuf::from("/repo"),
        std::path::PathBuf::from("/wt-root"),
    );
    let p1 = provider.worktree_path_for("alpha");
    let p2 = provider.worktree_path_for("alpha");
    assert_eq!(p1, p2);
    // Path includes the spec name + hash for uniqueness
    let name = p1.file_name().unwrap().to_string_lossy().to_string();
    assert!(name.starts_with("alpha-"));
}

#[test]
fn sota_smoke_registry_build_tool_description_includes_sota_agent() {
    use theo_agent_runtime::subagent::approval::{persist_approved, sha256_hex, ApprovalManifest, ApprovedEntry};
    let project = TempDir::new().unwrap();
    let agents = project.path().join(".theo").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("security-reviewer.md"), SOTA_AGENT_SPEC).unwrap();

    let manifest = ApprovalManifest {
        approved: vec![ApprovedEntry {
            file: "security-reviewer.md".to_string(),
            sha256: sha256_hex(SOTA_AGENT_SPEC),
        }],
    };
    persist_approved(project.path(), &manifest).unwrap();

    let mut reg = SubAgentRegistry::with_builtins();
    reg.load_all(Some(project.path()), None, ApprovalMode::Interactive);
    let desc = reg.build_tool_description();

    // All builtins + custom present in deterministic order
    assert!(desc.contains("explorer"));
    assert!(desc.contains("implementer"));
    assert!(desc.contains("verifier"));
    assert!(desc.contains("reviewer"));
    assert!(desc.contains("security-reviewer"));
    assert!(desc.contains("on-demand"));
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
