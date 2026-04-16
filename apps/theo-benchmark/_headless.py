#!/usr/bin/env python3
"""
Headless executor — thin wrapper around `theo --headless`.

Every benchmark module that needs to run the agent MUST go through this
module. No Python reimplementation of the agent loop is allowed.

The Rust binary owns the full agent lifecycle:
  - Tool execution (read, edit, bash, grep, etc.)
  - LLM inference and tool-call parsing
  - State machine (Planning → Executing → Evaluating → Converged)
  - Context engineering (GRAPHCTX)
  - Convergence detection and iteration limits

This module only handles:
  - Invoking the binary with the right flags
  - Parsing the JSON output (schema: theo.headless.v1)
  - Retry on transient errors (rate limits)
  - Returning structured results
"""

from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

DEFAULT_BIN = Path(__file__).resolve().parents[2] / "target" / "release" / "theo"


def _resolve_bin() -> Path:
    """Resolve theo binary path from env or default."""
    env = os.environ.get("THEO_BIN", "")
    if env:
        return Path(env)
    return DEFAULT_BIN


# ---------------------------------------------------------------------------
# Result types
# ---------------------------------------------------------------------------


@dataclass
class HeadlessResult:
    """Parsed output from `theo --headless`."""

    success: bool = False
    summary: str = ""
    iterations: int = 0
    duration_ms: int = 0
    tokens_input: int = 0
    tokens_output: int = 0
    tokens_total: int = 0
    tool_calls_total: int = 0
    tool_calls_success: int = 0
    llm_calls: int = 0
    llm_retries: int = 0
    files_edited: list[str] = field(default_factory=list)
    model: str = ""
    mode: str = ""
    provider: str = ""
    exit_code: int = -1
    error: Optional[str] = None
    raw_json: Optional[dict] = None

    @classmethod
    def from_json(cls, data: dict, exit_code: int = 0) -> HeadlessResult:
        tokens = data.get("tokens") or {}
        tools = data.get("tools") or {}
        llm = data.get("llm") or {}
        files = data.get("files_edited") or []
        if isinstance(files, int):
            files = []

        return cls(
            success=data.get("success", False),
            summary=data.get("summary", ""),
            iterations=data.get("iterations", 0),
            duration_ms=data.get("duration_ms", 0),
            tokens_input=tokens.get("input", 0),
            tokens_output=tokens.get("output", 0),
            tokens_total=tokens.get("total", 0),
            tool_calls_total=tools.get("total", 0),
            tool_calls_success=tools.get("success", 0),
            llm_calls=llm.get("calls", 0),
            llm_retries=llm.get("retries", 0),
            files_edited=files if isinstance(files, list) else [],
            model=data.get("model", ""),
            mode=data.get("mode", ""),
            provider=data.get("provider", ""),
            exit_code=exit_code,
            raw_json=data,
        )

    @classmethod
    def from_error(cls, error: str, exit_code: int = -1) -> HeadlessResult:
        return cls(success=False, error=error, exit_code=exit_code)


# ---------------------------------------------------------------------------
# Core executor
# ---------------------------------------------------------------------------


def run_headless(
    prompt: str,
    repo: str | Path = ".",
    *,
    max_iter: int = 30,
    mode: str = "agent",
    timeout: int = 600,
    model: Optional[str] = None,
    provider: Optional[str] = None,
    theo_bin: Optional[Path] = None,
    env_extra: Optional[dict[str, str]] = None,
    retries: int = 3,
    retry_wait: int = 30,
) -> HeadlessResult:
    """Run `theo --headless` and return parsed result.

    This is the ONLY way benchmark code should invoke the agent.
    """
    bin_path = theo_bin or _resolve_bin()
    if not bin_path.exists():
        return HeadlessResult.from_error(f"Binary not found: {bin_path}")

    cmd = [
        str(bin_path),
        "--headless",
        "--repo", str(repo),
        "--mode", mode,
        "--max-iter", str(max_iter),
    ]
    if model:
        cmd.extend(["--model", model])
    if provider:
        cmd.extend(["--provider", provider])
    cmd.append(prompt)

    env = os.environ.copy()
    if env_extra:
        env.update(env_extra)

    last_error = ""
    for attempt in range(retries):
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                env=env,
            )

            # Check for rate limit
            combined = (proc.stdout or "") + (proc.stderr or "")
            if "rate limit" in combined.lower() and attempt < retries - 1:
                wait = retry_wait * (attempt + 1)
                time.sleep(wait)
                last_error = f"rate limited (attempt {attempt + 1})"
                continue

            # Parse JSON from stdout (last JSON line)
            headless_json = _parse_json_line(proc.stdout)
            if headless_json:
                return HeadlessResult.from_json(headless_json, proc.returncode)

            # No JSON — return error with stderr context
            stderr_tail = (proc.stderr or "")[-500:]
            return HeadlessResult.from_error(
                f"No JSON output (exit={proc.returncode}): {stderr_tail}",
                exit_code=proc.returncode,
            )

        except subprocess.TimeoutExpired:
            return HeadlessResult.from_error(f"Timeout after {timeout}s")
        except Exception as e:
            last_error = str(e)
            if attempt < retries - 1:
                time.sleep(retry_wait)
                continue
            return HeadlessResult.from_error(f"Exception: {e}")

    return HeadlessResult.from_error(f"All {retries} retries exhausted: {last_error}")


def _parse_json_line(stdout: str) -> Optional[dict]:
    """Find and parse the last JSON line from stdout."""
    if not stdout:
        return None
    for line in reversed(stdout.splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


# ---------------------------------------------------------------------------
# Convenience: agent_fn compatible with TaskEngine
# ---------------------------------------------------------------------------


def make_agent_fn(
    repo_path: str,
    *,
    max_iter: int = 30,
    timeout: int = 600,
    theo_bin: Optional[Path] = None,
):
    """Create an agent_fn callback compatible with TaskEngine.execute_spec().

    Returns a function with signature:
        (description: str, context: str) -> (success: bool, result: str, error: str)
    """

    def agent_fn(description: str, context: str) -> tuple[bool, str, str]:
        full_prompt = f"{context}\n\nTASK: {description}" if context else description
        result = run_headless(
            full_prompt,
            repo=repo_path,
            max_iter=max_iter,
            timeout=timeout,
            theo_bin=theo_bin,
        )
        if result.success:
            return True, result.summary, ""
        return False, "", result.error or result.summary or "Agent did not converge"

    return agent_fn
