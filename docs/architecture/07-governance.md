# 07 — Governance (`theo-governance`)

Lightweight policy engine sitting in the critical path. Performs risk assessment, sandbox policy generation, toxic command sequence detection, session quality metrics, and append-only audit trail.

This bounded context operationalizes the harness-engineering principle that reliability comes from a *combination* of feedforward guides and feedback sensors, not from trusting the model alone. Governance is where Theo's **cybernetic regulator** lives — it watches the stream of agent actions, categorizes risk, and keeps the system inside architectural and operational constraints.

### Mapping to the Three Regulation Categories

From `docs/pesquisas/harness-engineering.md`, every harness regulates one of three dimensions. `theo-governance` contributes to all three:

| Category | Theo governance controls |
|---|---|
| **Maintainability** | Session quality metrics (`context_hit_rate`, `over_read_ratio`), wiki linter (via `theo-engine-retrieval`), audit trail drift detection |
| **Architecture fitness** | Risk alerts on impact reports (high-impact community, core module touches), untested-modification alerts, dependency-direction checks — see §Architecture Fitness Enforcement below |
| **Behaviour** | Sandbox policy per command risk, toxic sequence analyzer, convergence / done gate signals (consumed by runtime) |

Dependencies: `theo-domain`, `serde`, `serde_json`, `thiserror`.

## Module Map

```
theo-governance/src/
├── lib.rs                # Re-exports
├── alerts.rs             # Risk alert generation from impact reports
├── impact.rs             # Re-export of theo_domain::graph_context::ImpactReport
├── metrics.rs            # Session quality metrics
├── sandbox_audit.rs      # Append-only JSONL audit trail
├── sandbox_policy.rs     # Per-command SandboxConfig generation
└── sequence_analyzer.rs  # Multi-command toxic pattern detection
```

## Sandbox Policy Generation (sandbox_policy.rs)

Generates a `SandboxConfig` per command based on risk assessment:

```rust
pub fn assess_risk(command: &str) -> CommandRisk;
pub fn generate_config(command: &str, project_dir: &Path) -> SandboxConfig;
```

| Risk Level | Triggers | Policy |
|---|---|---|
| `Low` | `ls`, `cat`, `echo`, `pwd` | Read-only filesystem, no network |
| `Medium` | `git`, `cargo test`, `npm install` | Project dir write, limited network |
| `High` | `curl`, `wget`, `pip install` | Project dir write, restricted network |
| `Critical` | `rm -rf`, `chmod`, `sudo`, `dd` | Blocked or heavily restricted |

These policies are **guides**: they constrain possible actions up front and encode organizational risk tolerance into the runtime.

## Sequence Analyzer (sequence_analyzer.rs)

Detects multi-command attack patterns by analyzing sequences of bash commands:

```rust
pub fn analyze_sequence(commands: &[&str], patterns: &[ToxicPattern]) -> SequenceVerdict;
```

### Built-in Toxic Patterns (6)

| Pattern | Description | Required Keywords |
|---|---|---|
| `payload_drop` | Download and execute remote payload | `curl`/`wget` + `chmod`/`sh`/`bash` |
| `exfil_via_file` | Exfiltrate data through file operations | `tar`/`zip` + `curl`/`scp`/`rsync` |
| `git_force_push` | Force push to remote | `git push` + `--force`/`-f` |
| `ssh_key_exfil` | SSH key theft | `.ssh` + `cat`/`cp`/`scp` |
| `env_exfil` | Environment variable exfiltration | `env`/`printenv` + `curl`/`nc`/`base64` |
| `reverse_shell` | Reverse shell establishment | `nc`/`bash -i`/`/dev/tcp` + `exec`/`>&` |

Returns `SequenceVerdict::Toxic { pattern_name, description }` if detected.

This is a hybrid control: it inspects behavior after individual commands are proposed but before the sequence is allowed to accumulate into a dangerous pattern.

## Risk Alerts (alerts.rs)

Generates alerts from GRAPHCTX `ImpactReport`:

```rust
pub fn generate_alerts(report: &ImpactReport) -> Vec<RiskAlert>;
pub fn check_untested_modifications(symbols: &[String], coverage: &[String]) -> Vec<RiskAlert>;
```

| Alert Level | Trigger |
|---|---|
| `Info` | Low-impact community affected |
| `Warning` | Untested symbols modified, medium-impact community |
| `Critical` | High-impact community (>50 nodes), core module affected |

Risk alerts convert structural context into review pressure. They are especially useful when agent throughput exceeds human review bandwidth.

## Session Quality Metrics (metrics.rs)

```rust
pub struct SessionMetrics {
    pub context_hit_rate: f64,    // % of retrieved context actually used by agent
    pub context_miss_rate: f64,   // % of needed context not retrieved
    pub over_read_ratio: f64,     // retrieved but unused / total retrieved
    pub cluster_coverage: f64,    // % of relevant clusters covered

    // --- Sensor-coverage (roadmap — see §Sensor Coverage below) ---
    pub sensor_fire_rate: f64,    // sensors fired / write-tool calls that should have fired
    pub sensor_block_rate: f64,   // runs where a sensor caught an issue / total runs
    pub mean_sensor_latency_ms: f64,
}
```

These metrics are feedback sensors over harness quality itself. They answer questions such as:

- Did retrieval surface the right context?
- Did the agent over-read or under-read?
- Are structural guards helping, or just adding noise?
- **Are the sensors actually firing?** (See §Sensor Coverage.)

## Audit Trail (sandbox_audit.rs)

Append-only JSONL audit log of all sandboxed command executions:

```rust
pub struct AuditTrail {
    records: Mutex<Vec<SandboxAuditRecord>>,
    file_path: Option<PathBuf>,  // .theo/audit.jsonl
}

pub struct SandboxAuditRecord {
    pub timestamp: String,
    pub command: String,
    pub config_applied: SandboxConfig,
    pub risk_level: CommandRisk,
    pub success: bool,
    pub exit_code: i32,
    pub violations: Vec<SandboxViolation>,
    pub executor_entries: Vec<AuditEntry>,
}
```

Thread-safe, append on every bash execution. Survives process crash via JSONL format.

The audit trail is both a forensic artifact and a continuity artifact for later agent or human review.

## Entropy and Garbage Collection

From `docs/pesquisas/harness-engineering-openai.md`: *"Codex replicates patterns that already exist in the repository — even uneven or suboptimal ones. Over time, this inevitably leads to drift."* OpenAI's mitigation is encoding **"golden principles"** into the repository plus recurring cleanup runs that scan for deviations and open refactoring PRs.

Theo's equivalent mechanisms — with honest status tags:

| Concern | Mechanism | Status |
|---|---|---|
| Golden principles | `.theo/theo.md`, `docs/architecture/`, skill definitions | **Implemented** (repository) |
| Drift detection (structural) | Wiki linter (`wiki/lint.rs`) | **Implemented** — stale pages, broken links |
| Drift detection (behavioral) | `over_read_ratio` trend, audit trail pattern analysis | **Partial** — metrics collected, trend analysis not wired |
| Cleanup runs (on-demand) | `theo pilot` against a roadmap | **Implemented** |
| Cleanup runs (continuous crawler opening refactor PRs) | — | **Gap** — OpenAI `§10` equivalent not built |
| Quality grades over time | Session metrics + benchmark scores | **Partial** — collected per run, not aggregated over time |

The invariant **technical debt is paid down continuously in small increments, never in painful bursts** is aspirational today: Theo has the *on-demand* vehicle (`theo pilot`) but not the *continuous, autonomous* drift-scanner agent. Bridging that gap is one of the roadmap items listed in `README.md` → Gaps vs Research.

## Sensor Coverage — Roadmap, Not Delivered

The Böckeler article explicitly flags: *"If sensors never fire, is that a sign of high quality or inadequate detection mechanisms?"*

Theo does not yet have a delivered sensor-coverage metric analogous to mutation testing for tests. The `SessionMetrics` fields `sensor_fire_rate` / `sensor_block_rate` / `mean_sensor_latency_ms` listed above are the **target schema**; they are emitted by `SensorRunner` (`theo-agent-runtime/src/sensor.rs`) as `SensorExecuted` events but not yet aggregated into the governance layer's metrics output.

The plan in concrete terms:

1. Define "should-have-fired" — a write-tool call where at least one registered sensor matched the tool category and the file glob.
2. Aggregate at run-end (inside `record_session_exit`) and emit as part of the headless JSON output (`theo.headless.v2`).
3. Track a trend across benchmark runs — a drop in `sensor_fire_rate` without a drop in `sensor_block_rate` implies either slop sensors or over-triggering; the inverse implies blind spots.

Until this lands, the docs do **not** claim sensor-coverage parity with mutation-tested test suites. This is an open capability, listed in `README.md` → Gaps vs Research.

## Architecture Fitness Enforcement — Compile-Time Today, Runtime Sensor Tomorrow

The README lists an **inviolable dependency direction** (`theo-domain` → nothing, apps → `theo-application` only, etc.). Today this is enforced exclusively by the Rust compiler through `Cargo.toml` — any violation is caught at `cargo build` time but only visible *after* the agent has already written the code.

OpenAI `§6` describes a more active pattern: *"custom linters and structural tests [...] we write the error messages to inject remediation instructions into agent context."* The error message itself is a guide — it tells the agent *how* to fix the violation on the next turn.

Theo has none of these today. The concrete gap:

| Check | Today | Target |
|---|---|---|
| Dependency direction (e.g., `theo-domain` → no other crate) | `cargo build` error at compile-time | `arch-lint` tool in registry, runs after every edit, returns LLM-targeted remediation |
| Tool trait contract (`schema()` + `category()` declared) | Unenforced (trivial to break) | Structural test asserted at runtime, surfaced as sensor |
| Plan-mode write gating (`AgentMode::Plan` only writes to `.theo/plans/`) | Runtime check in `RunEngine` | OK — already a runtime sensor |
| File-size / function-size invariants | Unenforced | Custom linter, remediation prose in error |

The roadmap is to introduce an `arch-lint` tool (and eventually a standalone crate) that runs *per turn* and whose error messages are deliberately shaped for LLM self-correction — the same "positive prompt injection" pattern `06-tooling.md` documents for other tools.

## Shift-Left: Timing of Harness Controls

Böckeler `§Timing` prescribes distributing guides and sensors across the change lifecycle by cost and criticality. Theo's placement today:

| Timing | Control | Implementation |
|---|---|---|
| **Pre-tool-call** (guide, cheap) | Sandbox policy, capability gate, command validator, toxic sequence analyzer | `theo-governance` + `theo-tooling::sandbox` |
| **Intra-turn** (sensor, cheap) | Edit-verify hook, convergence evaluator, done gate | `theo-agent-runtime::sensor`, `.theo/hooks/` |
| **End-of-run** (sensor, medium) | `cargo test -p <crate>`, session quality metrics, audit trail flush | `theo-agent-runtime::run_engine`, this crate |
| **Continuous / out-of-band** (sensor, expensive) | Wiki linter, benchmark evolve loop, pilot cleanup (gap — not yet a crawler) | `theo-engine-retrieval::wiki/lint.rs`, `apps/theo-benchmark/runner/evolve.py` |

The fourth row is the weakest link: OpenAI's `§10` describes a **continuous drift-scanner agent** that opens refactor PRs on a cadence. Theo's `PilotLoop` runs on-demand against a roadmap, not as a background crawler. Closing this gap is tracked with the other roadmap items in the README.

## Merge Philosophy (not yet a documented project policy)

OpenAI `§7` makes an explicit choice: *"minimal blocking merge gates; pull requests are short-lived; test flakes are often addressed with follow-up runs rather than blocking progress indefinitely."* The tradeoff (cheap corrections, expensive waiting) only makes sense when agent throughput dominates human attention.

Theo has not yet adopted an explicit merge philosophy. The governance crate provides the *substrate* (risk alerts, audit trail, impact report) to make such a policy enforceable, but the policy itself — how many blocking gates, how test flakes are handled, when humans must review — is not captured in any ADR today. This is flagged so a future ADR can close it.
