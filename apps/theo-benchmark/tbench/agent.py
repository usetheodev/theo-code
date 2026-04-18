"""
Theo Code agent adapter for Terminal-Bench / Harbor (2026 API).

Implements BaseInstalledAgent so Harbor can install and invoke Theo inside
each task container. The agent runs in `--headless` mode — the Rust binary
owns the full agent lifecycle (tools, LLM calls, convergence).

Usage:
    # Standard Terminal-Bench evaluation
    tb run --agent-import-path tbench.agent:TheoAgent \\
           --dataset-name terminal-bench-core --dataset-version 0.1.1

    # With parallelism
    tb run --agent-import-path tbench.agent:TheoAgent \\
           --dataset-name terminal-bench-core --dataset-version 0.1.1 \\
           --n-concurrent 8

Requires:
    - THEO_BIN_URL or /mnt/theo-bin/theo for binary distribution
    - API keys via OPENAI_API_KEY / ANTHROPIC_API_KEY
"""

from __future__ import annotations

import json
import os
from pathlib import Path

try:
    from harbor.agents import BaseInstalledAgent
except ImportError:
    try:
        from terminal_bench.agents import AbstractInstalledAgent as BaseInstalledAgent
    except ImportError:
        from typing import Any

        class BaseInstalledAgent:  # type: ignore[no-redef]
            """Stub for development outside Harbor."""
            pass


class TheoAgent(BaseInstalledAgent):
    """Harbor agent adapter for Theo Code.

    Lifecycle:
      1. Harbor calls install() → runs setup.sh to place `theo` in PATH
      2. Harbor calls run() → executes `theo --headless` with instruction
      3. After run, populate_context_post_run() parses headless JSON output
    """

    @staticmethod
    def name() -> str:
        return "theo-code"

    @staticmethod
    def version() -> str:
        return "0.1.0"

    @property
    def _install_agent_script_path(self) -> Path:
        return Path(__file__).parent / "setup.sh"

    async def install(self, environment) -> None:
        """Install theo binary inside the container."""
        script = Path(__file__).parent / "setup.sh"
        await environment.exec_as_root(f"bash {script}")

    async def run(self, instruction: str, environment, context) -> None:
        """Run theo --headless with the task instruction.

        The Rust binary handles everything: tools, LLM, convergence.
        We only need to invoke it and capture the result.
        """
        escaped = instruction.replace("'", "'\\''")
        max_iter = int(os.environ.get("THEO_MAX_ITER", "50"))

        cmd = (
            f"theo --headless --max-iter {max_iter} "
            f"'{escaped}' "
            f"2>/tmp/theo-stderr.log"
        )

        # Execute as agent user — Harbor handles logging and context population
        await environment.exec_as_agent(
            cmd,
            timeout=float("inf"),
            block=True,
        )

    async def populate_context_post_run(self, context) -> None:
        """Parse headless JSON output into Harbor context."""
        try:
            # Read stdout captured by Harbor
            stdout = context.get("stdout", "")
            for line in reversed(stdout.splitlines()):
                line = line.strip()
                if line.startswith("{"):
                    try:
                        data = json.loads(line)
                        if data.get("schema") == "theo.headless.v1":
                            context["success"] = data.get("success", False)
                            context["summary"] = data.get("summary", "")
                            context["iterations"] = data.get("iterations", 0)
                            context["tokens"] = data.get("tokens", {})
                            context["tools"] = data.get("tools", {})
                            context["files_edited"] = data.get("files_edited", [])
                            return
                    except json.JSONDecodeError:
                        continue
        except Exception:
            pass

    # Legacy API support (AbstractInstalledAgent)
    def _run_agent_commands(self):
        """Fallback for older Harbor versions using AbstractInstalledAgent."""
        try:
            from terminal_bench.agents import TerminalCommand
        except ImportError:
            return []

        desc = getattr(self, "task_description", "Complete the task described in instruction.md")
        escaped = desc.replace("'", "'\\''")
        max_iter = int(os.environ.get("THEO_MAX_ITER", "50"))
        return [
            TerminalCommand(
                command=(
                    f"theo --headless --max-iter {max_iter} "
                    f"'{escaped}' "
                    f"2>/tmp/theo-stderr.log"
                ),
                max_timeout_sec=float("inf"),
                block=True,
            )
        ]

    @property
    def _env(self) -> dict[str, str]:
        """Environment variables forwarded into the container."""
        env: dict[str, str] = {}
        for key in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "THEO_BIN_URL",
            "THEO_MODEL",
            "THEO_PROVIDER",
            "THEO_MAX_ITER",
        ]:
            val = os.environ.get(key, "")
            if val:
                env[key] = val
        # Forward OAuth token store if present
        home = os.environ.get("HOME", "/root")
        token_path = os.path.join(home, ".config", "theo", "openai_tokens.json")
        if os.path.exists(token_path):
            env["THEO_OAUTH_TOKENS_PATH"] = token_path
        return env
