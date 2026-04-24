"""
Theo Code agent adapter for Terminal-Bench (2026 API).

Implements AbstractInstalledAgent (terminal-bench >= 0.2) so the harness
can install and invoke Theo inside each task container. The agent runs
in `--headless` mode — the Rust binary owns the full agent lifecycle
(tools, LLM calls, convergence, OTLP export).

Usage:
    # Standard Terminal-Bench evaluation
    tb run --agent-import-path tbench.agent:TheoAgent \\
           --dataset-name terminal-bench-core --dataset-version head

    # With parallelism
    tb run --agent-import-path tbench.agent:TheoAgent \\
           --dataset-name terminal-bench-core --dataset-version head \\
           --n-concurrent 8

Requires:
    - THEO_BIN_URL or /mnt/theo-bin/theo for binary distribution
    - API keys via OPENAI_API_KEY / ANTHROPIC_API_KEY (or OAuth Codex token)

Phase 46 (benchmark-validation-plan):
    - version() now embeds the git SHA of the theo source tree
    - run() forwards OTLP_* env vars so spans reach the local collector
    - populate_context_post_run() computes cost_usd from pricing.toml
"""

from __future__ import annotations

import json
import os
import shlex
import subprocess
from pathlib import Path

# Phase 46: terminal-bench 0.2+ exposes AbstractInstalledAgent + TerminalCommand.
# We pin to that contract; legacy harbor fallback removed (was never wired).
from terminal_bench.agents.installed_agents.abstract_installed_agent import (  # type: ignore
    AbstractInstalledAgent,
)
# TerminalCommand lives in terminal.models in tb 0.2+; older versions had
# it under harness_models. Try the new path first, fall back to old.
try:
    from terminal_bench.terminal.models import TerminalCommand  # type: ignore
except ImportError:  # pragma: no cover
    from terminal_bench.harness_models import TerminalCommand  # type: ignore

# Phase 46: compute_cost from pricing.toml. Import path resolves whether the
# adapter runs from `apps/theo-benchmark` (dev) or installed system-wide.
import sys as _sys
_HERE = Path(__file__).resolve().parent
_BENCH_ROOT = _HERE.parent
if str(_BENCH_ROOT) not in _sys.path:
    _sys.path.insert(0, str(_BENCH_ROOT))
try:
    from pricing import compute_cost  # type: ignore
except ImportError:
    def compute_cost(_ti: int, _to: int, _m: str) -> float:
        return 0.0


def _git_sha_short() -> str:
    """Return short SHA of the theo source tree, or 'unknown' if not in git."""
    try:
        # Try theo repo root (3 levels up from this file: tbench → bench → apps → repo)
        repo = Path(__file__).resolve().parents[3]
        out = subprocess.check_output(
            ["git", "-C", str(repo), "rev-parse", "--short", "HEAD"],
            stderr=subprocess.DEVNULL,
            timeout=5,
        )
        return out.decode().strip()
    except Exception:
        return "unknown"


_OTLP_ENV_KEYS = (
    "OTLP_ENDPOINT",
    "OTLP_PROTOCOL",
    "OTLP_HEADERS",
    "OTLP_SERVICE_NAME",
    "OTLP_TIMEOUT_SECS",
    "OTLP_BATCH_SIZE",
)


class TheoAgent(AbstractInstalledAgent):
    """Terminal-Bench agent adapter for Theo Code.

    Phase 46 (benchmark-validation-plan): pinned to terminal-bench >= 0.2
    `AbstractInstalledAgent` API — no fallback shims. setup.sh installs
    the theo binary; `_run_agent_commands` shells out to `theo --headless`
    with OTLP env vars forwarded.

    Lifecycle:
      1. Harness copies `setup.sh` into container + runs as root
      2. Harness invokes `_run_agent_commands(task_description)` → list[TerminalCommand]
      3. After run, harness reads stdout — we expose `parse_result(stdout)`
         as a class method so the runner script can compute cost_usd outside
         the container (post-process step).
    """

    SCHEMA_VERSION = "theo.headless.v2"

    @staticmethod
    def name() -> str:
        return "theo-code"

    @staticmethod
    def version() -> str:
        return f"0.1.0+{_git_sha_short()}"

    def __init__(self, model_name: str | None = None, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._model_name = model_name or os.environ.get("THEO_MODEL")

    @property
    def _install_agent_script_path(self) -> os.PathLike:
        return Path(__file__).parent / "setup.sh"

    @property
    def _env(self) -> dict[str, str]:
        """Environment variables forwarded into the container.

        Phase 46 (benchmark-validation-plan): OTLP_* vars forwarded so
        `theo --headless` can export spans to a collector reachable from
        inside the container.
        """
        env: dict[str, str] = {}
        # API keys + theo runtime config
        for key in (
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "THEO_BIN_URL",
            "THEO_AUTH_URL",
            "THEO_MODEL",
            "THEO_PROVIDER",
            "THEO_MAX_ITER",
            "THEO_SKIP_ONBOARDING",  # bench: skip user-prompt before tool use
            "THEO_FORCE_TOOL_CHOICE",  # bench: force specific tool dispatch
            "THEO_TEMPERATURE",
            "THEO_BENCHMARK_MODE",  # bug #3: relax safety for benign bench tasks
        ):
            val = os.environ.get(key, "")
            if val:
                env[key] = val
        if self._model_name:
            env["THEO_MODEL"] = self._model_name
        # Phase 46: OTLP wiring
        for key in _OTLP_ENV_KEYS:
            val = os.environ.get(key, "")
            if val:
                env[key] = val
        # Forward OAuth token store if present
        home = os.environ.get("HOME", "/root")
        token_path = os.path.join(home, ".config", "theo", "auth.json")
        if os.path.exists(token_path):
            env["THEO_AUTH_PATH"] = token_path
        return env

    def _run_agent_commands(self, task_description: str) -> list[TerminalCommand]:
        """Build the single command that invokes theo --headless.

        Quote the task description with shlex.quote — handles single
        quotes, backticks, etc. in instruction text correctly.
        Phase 47: stdout redirected to /tmp/theo-stdout.log so perform_task()
        below can copy it out before container teardown.

        Bug #6 fix: default max_iter bumped from 20 to 35. Data from the
        first run showed 64% of trials hit max_iter=20 mid-implementation.
        35 fits 95th percentile of observed completion (resolved trials
        used 6-21 iters; 35 gives headroom for hard tasks).
        """
        max_iter = int(os.environ.get("THEO_MAX_ITER", "35"))
        quoted = shlex.quote(task_description)
        cmd = (
            f"theo --headless --max-iter {max_iter} {quoted} "
            f"2>/tmp/theo-stderr.log | tee /tmp/theo-stdout.log"
        )
        return [
            TerminalCommand(
                command=cmd,
                max_timeout_sec=float("inf"),
                block=True,
            )
        ]

    def perform_task(self, instruction, session, logging_dir=None):
        """Phase 47 instrumentation override.

        Wraps super().perform_task() to:
          1. Run the agent normally (super)
          2. Copy /tmp/theo-{stdout,stderr}.log + auth.json from container
             to logging_dir (so they survive container teardown)
          3. Parse the headless JSON line from stdout
          4. Compute cost_usd via pricing.toml
          5. Write a sidecar `theo-headless.json` with the full enriched payload
          6. Set total_input_tokens / total_output_tokens on the AgentResult
             so tb's own results.json carries them (no more zeros)

        Errors at any post-step are LOGGED but never abort the run —
        we want telemetry to be best-effort, never break the test.
        """
        # 1. Normal flow
        result = super().perform_task(instruction, session, logging_dir=logging_dir)

        # 2-6. Telemetry capture — wrapped in try so a failure here never
        # breaks the harness's ability to record the test verdict.
        try:
            self._capture_telemetry(session, logging_dir, result)
        except Exception as e:
            # Best effort — write a marker file so the analyzer knows
            # capture failed for this trial.
            if logging_dir is not None:
                try:
                    (Path(logging_dir) / "theo-telemetry-error.txt").write_text(
                        f"telemetry capture failed: {type(e).__name__}: {e}\n"
                    )
                except Exception:
                    pass
        return result

    def _capture_telemetry(self, session, logging_dir, result) -> None:
        """Copy theo logs out of container + parse JSON + sidecar."""
        if logging_dir is None:
            return
        log_dir = Path(logging_dir)
        log_dir.mkdir(parents=True, exist_ok=True)

        # Copy stdout/stderr OUT of container before teardown.
        # session.container is the Docker container handle (terminal-bench
        # exposes it on TmuxSession).
        for src, dst in [
            ("/tmp/theo-stdout.log", "theo-stdout.log"),
            ("/tmp/theo-stderr.log", "theo-stderr.log"),
        ]:
            try:
                # Try docker SDK exec first, fall back to capture_pane
                import subprocess as _sp
                container_name = getattr(session.container, "name", None) or \
                                 getattr(session.container, "id", None)
                if container_name:
                    out = _sp.check_output(
                        ["docker", "exec", container_name, "cat", src],
                        timeout=30,
                    )
                    (log_dir / dst).write_bytes(out)
            except Exception:
                pass

        # Also dump the captured tmux pane as last-resort fallback for stdout
        try:
            pane = session.capture_pane(capture_entire=True)
            (log_dir / "tmux-pane.log").write_text(pane)
        except Exception:
            pass

        # Parse headless JSON from theo-stdout.log → sidecar with cost
        stdout_path = log_dir / "theo-stdout.log"
        if not stdout_path.exists():
            # Fall back to tmux pane
            stdout_path = log_dir / "tmux-pane.log"
        if stdout_path.exists():
            payload = self.parse_result(stdout_path.read_text(errors="replace"))
            if payload:
                # Add provenance
                payload["adapter_version"] = self.version()
                (log_dir / "theo-headless.json").write_text(
                    json.dumps(payload, indent=2)
                )
                # Backfill the AgentResult so tb's results.json carries
                # tokens (instead of always-zero).
                tok = payload.get("tokens", {}) or {}
                try:
                    result.total_input_tokens = int(tok.get("input", 0) or 0)
                    result.total_output_tokens = int(tok.get("output", 0) or 0)
                except Exception:
                    pass

    @classmethod
    def parse_result(cls, stdout: str) -> dict:
        """Extract the headless JSON line from stdout + enrich with cost_usd.

        Used by the post-run analysis script. Returns a dict with
        success/summary/iterations/tokens/tools + cost_usd derived from
        pricing.toml. Returns an empty dict when no valid line is found.
        """
        for line in reversed(stdout.splitlines()):
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                data = json.loads(line)
            except json.JSONDecodeError:
                continue
            schema = data.get("schema", "")
            if not schema.startswith("theo.headless"):
                continue
            tokens = data.get("tokens", {}) or {}
            model = data.get("model", "") or os.environ.get("THEO_MODEL", "")
            data["cost_usd"] = compute_cost(
                int(tokens.get("input", 0) or 0),
                int(tokens.get("output", 0) or 0),
                model,
            )
            data["adapter_version"] = cls.version()
            return data
        return {}
