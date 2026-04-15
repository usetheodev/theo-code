"""
Theo Code agent adapter for Terminal-Bench / Harbor.

Implements AbstractInstalledAgent so Harbor can install and invoke
Theo inside each task container. The agent is invoked with --headless
which reads the task instruction, runs autonomously, and exits.

Usage with Harbor:
    harbor run -d terminal-bench/terminal-bench-2 -a theo-code

Requires:
    - THEO_BIN_URL or /mnt/theo-bin/theo for binary distribution
    - OpenAI OAuth tokens in the container (via env vars)
"""

from __future__ import annotations

import os
from pathlib import Path

try:
    from terminal_bench.agents import AbstractInstalledAgent, TerminalCommand
except ImportError:
    # Graceful degradation when terminal_bench is not installed
    # (e.g., during development/testing outside Harbor)
    from typing import Any

    class AbstractInstalledAgent:  # type: ignore[no-redef]
        pass

    class TerminalCommand:  # type: ignore[no-redef]
        def __init__(self, **kwargs: Any):
            self.__dict__.update(kwargs)


class TheoAgent(AbstractInstalledAgent):
    """Harbor agent adapter for Theo Code."""

    @staticmethod
    def name() -> str:
        return "theo-code"

    @property
    def _install_agent_script_path(self) -> Path:
        return Path(__file__).parent / "setup.sh"

    def _run_agent_commands(self) -> list[TerminalCommand]:
        # The task_description is set by Harbor from instruction.md.
        # We pass it as the prompt to theo --headless.
        # --max-iter 50 is generous but prevents infinite loops.
        desc = getattr(self, "task_description", "Complete the task described in instruction.md")
        escaped = desc.replace("'", "'\\''")
        return [
            TerminalCommand(
                command=(
                    f"theo --headless --max-iter 50 "
                    f"'{escaped}' "
                    f"2>/tmp/theo-stderr.log"
                ),
                max_timeout_sec=float("inf"),
                block=True,
            )
        ]

    @property
    def _env(self) -> dict[str, str]:
        env: dict[str, str] = {}
        # Forward OAuth/API credentials
        for key in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "THEO_BIN_URL",
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
