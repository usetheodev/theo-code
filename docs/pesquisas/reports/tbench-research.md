---
type: report
question: "What is Terminal-Bench, how does it work technically, and how would Theo Code integrate?"
generated_at: 2026-04-15T12:00:00-03:00
confidence: 0.88
sources_used: 14
---

# Report: Terminal-Bench (tbench.ai) Technical Research

## Executive Summary

Terminal-Bench is the de facto standard benchmark for evaluating AI coding agents in real terminal environments. Created by the Laude Institute (Stanford-affiliated), it consists of 89 tasks (v2.0) spanning software engineering, biology, security, and gaming. Each task is a Docker container with an instruction, environment, test script, and oracle solution. The evaluation harness (now called **Harbor**) orchestrates containerized agent execution and outcome-based verification. Integration requires implementing either a Python `BaseAgent` interface or an `AbstractInstalledAgent` that gets installed inside the task container. Theo Code could integrate as an installed agent with minimal adapter code.

## Analysis

### Finding 1: Task Format Specification

Each Terminal-Bench task is a directory with the following structure (Harbor task format):

```
<task-name>/
  instruction.md          # Natural language task description
  task.toml               # Configuration and metadata
  environment/
    Dockerfile             # or docker-compose.yaml
  solution/
    solve.sh               # Human-written oracle solution
  tests/
    test.sh                # Verification script
```

**task.toml schema fields** (from Harbor docs [S1]):

| Section | Field | Purpose |
|---------|-------|---------|
| `task` | `name`, `description`, `authors`, `keywords` | Metadata |
| `agent` | `timeout_sec`, `user` | Agent execution limits |
| `verifier` | `timeout_sec`, `env`, `user` | Test execution config |
| `solution` | `env` | Oracle solution variables |
| `environment` | `build_timeout_sec`, `docker_image`, `cpus`, `memory_mb`, `storage_mb`, `gpus`, `gpu_types`, `allow_internet`, `env`, `mcp_servers`, `healthcheck` | Container resources |

**Evaluation is outcome-driven**: tests verify the final container state (filesystem, process state, network config), not the commands the agent ran. This means any approach that achieves the correct end state passes [S2].

**Reward output**: Tests write either `/logs/verifier/reward.txt` (single numeric 0 or 1) or `/logs/verifier/reward.json` (structured metrics) [S1].

### Finding 2: Harness Architecture (Harbor)

Terminal-Bench has evolved through two harness generations:

1. **Terminal-Bench CLI (`tb`)** -- Original harness (v1.0), pip-installable as `terminal-bench`. Uses `tb run` commands. Still works for v1.x datasets.

2. **Harbor** -- New harness for v2.0+. Pip-installable as `harbor`. Supports cloud-deployed containers (Daytona, Modal), RL/SFT rollout interfaces, and massive parallelism (1000s of concurrent containers) [S3].

**Installation**:
```bash
uv tool install harbor    # or: pip install harbor
# Also requires: Docker
```

**Running an evaluation**:
```bash
# Local execution
harbor run -d terminal-bench/terminal-bench-2 -a oracle

# Cloud execution with Claude Code
export DAYTONA_API_KEY="..."
export ANTHROPIC_API_KEY="..."
harbor run \
  -d terminal-bench/terminal-bench-2 \
  -m anthropic/claude-haiku-4-5 \
  -a claude-code \
  --env daytona \
  -n 32
```

The harness handles: container orchestration, agent invocation, logging, timeout enforcement, test execution, and reward collection [S4].

### Finding 3: Agent Integration Interface

Three integration methods are supported [S5]:

**Method A: `AbstractInstalledAgent`** (recommended for CLI tools like Theo)

The agent is installed inside the task container via a setup script, then invoked with shell commands:

```python
from terminal_bench.agents import AbstractInstalledAgent

class TheoAgent(AbstractInstalledAgent):
    @staticmethod
    def name() -> str:
        return "theo-code"

    @property
    def _install_agent_script_path(self) -> Path:
        return Path("setup.sh")  # Copied into container, runs on Debian

    def _run_agent_commands(self) -> list[TerminalCommand]:
        return [
            TerminalCommand(
                command=f'theo -p "{self.task_description}" --non-interactive',
                max_timeout_sec=float("inf"),
                block=True,
            )
        ]

    @property
    def _env(self) -> dict[str, str]:
        return {"ANTHROPIC_API_KEY": os.environ["ANTHROPIC_API_KEY"]}
```

The `setup.sh` must install the agent on a bare Debian container.

**Method B: `BaseAgent`** (direct Python interface)

```python
from terminal_bench.agents import BaseAgent, AgentResult
from terminal_bench.terminal.tmux_session import TmuxSession

class TheoAgent(BaseAgent):
    @staticmethod
    def name() -> str:
        return "theo-code"

    def perform_task(
        self,
        task_description: str,
        session: TmuxSession,
        logging_dir: Path | None = None,
    ) -> AgentResult:
        # Send commands via tmux session
        session.send_keys("theo -p '...' --non-interactive")
        # ...
        return AgentResult(tokens_used=1234)
```

**Method C: MCP Server** -- Coming soon, not yet documented [S5].

**Key detail**: The agent interacts with a **tmux session** inside the Docker container. Commands are sent via tmux `send_keys`. The agent does NOT get direct stdin/stdout -- it operates through tmux, reading terminal output by capturing pane content [S5].

### Finding 4: How Existing Agents Integrate

From documentation and source references [S6, S7]:

- **Claude Code**: Invoked as `claude -p {description} --allowedTools "Bash Edit Write Read Glob Grep Agent"` inside the container. Uses `AbstractInstalledAgent`.
- **Codex CLI**: Similarly installed in container and invoked via CLI.
- **OpenHands**: Uses the `BaseAgent` interface with direct tmux control.
- **Terminus 2**: The "neutral testbed" agent -- single Bash tool only, designed to isolate model capability from agent scaffolding. Used as the baseline.

**Scaffolding impact**: The harness explicitly measures scaffolding quality by running the same model with different agents. Scaffolding contributes 2-6 percentage points [S8].

### Finding 5: Public Repositories and Resources

| Resource | URL | Status |
|----------|-----|--------|
| Terminal-Bench repo | https://github.com/laude-institute/terminal-bench | Public, Apache-2.0, 2000+ stars |
| Harbor framework | https://github.com/laude-institute/harbor | Public, 1.5k stars |
| Harbor docs | https://harborframework.com | Active |
| Terminal-Bench website | https://tbench.ai | Active |
| ArXiv paper | https://arxiv.org/abs/2601.11868 | Published Jan 2026 |
| PyPI (terminal-bench) | https://pypi.org/project/terminal-bench/ | Available |
| PyPI (harbor) | https://pypi.org/project/harbor/ | Available |
| Leaderboard | https://tbench.ai/leaderboard | Public, 115+ submissions |
| Leaderboard submission | PR to HuggingFace repo with logs | Documented in leaderboard README |

### Finding 6: Current Leaderboard (April 2026)

Terminal-Bench 2.0 top results [S8, S9]:

| Rank | Agent + Model | Score |
|------|---------------|-------|
| 1 | Claude Mythos Preview | 82.0% |
| 2 | Forge Code + Gemini 3.1 Pro | 78.4% |
| 3 | Factory Droid + GPT-5.3-Codex | 77.3% |
| 4 | GPT-5.4 (unknown scaffold) | 75.1% |
| 5 | Claude Opus 4.6 (Terminus 2) | 74.7% |
| -- | Claude Code (native scaffold) | 58.0% |

**No agent achieves 100% on all 89 tasks.** The benchmark remains challenging even for frontier models.

## Gaps

1. **Exact task.toml JSON schema** -- The docs describe fields but no formal JSON Schema or TOML schema file was found in the public repo.
2. **Submission format details** -- The leaderboard submission process references a HuggingFace PR workflow, but the exact log format and required metadata are not fully documented publicly.
3. **MCP integration** -- Listed as "coming soon" with no implementation details yet.
4. **Theo-specific integration complexity** -- Theo is a Rust binary, not a Python package. The `AbstractInstalledAgent` path requires a `setup.sh` that can install Theo's binary + dependencies on Debian. This is feasible but needs a portable build/release artifact.
5. **Cost per run** -- Not documented. With 89 tasks and potentially multiple retries, LLM API costs could be significant.

## Recommendations

### For Theo Code Integration

1. **Use `AbstractInstalledAgent`** -- This is the path Claude Code and Codex use. Write a `setup.sh` that downloads a pre-built Theo binary (from GitHub Releases or a CDN) and installs it on Debian.

2. **Implement a `--non-interactive` or `-p` prompt mode** -- The harness passes the task description as a string. Theo needs a mode where it accepts a prompt, executes autonomously, and exits when done (no TUI, no interactive input).

3. **Start with Terminus 2 baseline** -- Before building a full agent adapter, test Theo's underlying model routing through the Terminus 2 scaffold to establish a baseline score.

4. **Build the adapter in Python** -- The adapter is ~30 lines of Python (see Method A above). It lives outside the Theo Rust codebase, in a separate `theo-tbench-adapter` repo or directory.

5. **Target Terminal-Bench 2.0 via Harbor** -- v1.x is legacy. All new submissions use Harbor (`harbor run -d terminal-bench/terminal-bench-2`).

6. **Portable binary is prerequisite** -- The setup.sh must work on bare Debian. This means either:
   - Static Rust binary (musl target)
   - Or a release .deb package
   - Or a curl-to-install script pointing to GitHub Releases

## Sources

- [S1] Harbor Task Structure docs -- https://www.harborframework.com/docs/tasks
- [S2] ArXiv paper (HTML) -- https://arxiv.org/html/2601.11868v1
- [S3] Terminal-Bench 2.0 announcement -- https://www.tbench.ai/news/announcement-2-0
- [S4] Harbor running Terminal-Bench tutorial -- https://www.harborframework.com/docs/tutorials/running-terminal-bench
- [S5] Terminal-Bench agent introduction -- https://www.tbench.ai/docs/agent-introduction
- [S6] Terminal-Bench first steps -- https://www.tbench.ai/docs/first-steps
- [S7] Terminal-Bench CLAUDE.md -- https://github.com/laude-institute/terminal-bench/blob/main/CLAUDE.md
- [S8] Morph LLM Terminal-Bench 2.0 analysis -- https://www.morphllm.com/terminal-bench-2
- [S9] tbench.ai leaderboard -- https://www.tbench.ai/leaderboard
- [S10] Terminal-Bench GitHub repo -- https://github.com/laude-institute/terminal-bench
- [S11] Harbor GitHub repo -- https://github.com/laude-institute/harbor
- [S12] Terminal-Bench about page -- https://www.tbench.ai/about
- [S13] Terminal-Bench adapters docs -- https://www.tbench.ai/docs/adapters
- [S14] Artificial Analysis Terminal-Bench leaderboard -- https://artificialanalysis.ai/evaluations/terminalbench-hard
