#!/usr/bin/env python3
"""
Headless executor — thin wrapper around `theo --headless`.

Every benchmark module that needs to run the agent MUST go through this
module. No Python reimplementation of the agent loop is allowed.

The Rust binary owns the full agent lifecycle:
  - Tool execution (read, edit, bash, grep, etc.)
  - LLM inference and tool-call parsing
  - State machine (Planning -> Executing -> Evaluating -> Converged)
  - Context engineering (GRAPHCTX)
  - Convergence detection and iteration limits

This module only handles:
  - Invoking the binary with the right flags
  - Parsing the JSON output (schema: theo.headless.v1)
  - Retry on transient errors (rate limits)
  - Cost estimation from token counts
  - Multi-run aggregation with statistics
  - Returning structured results
"""

from __future__ import annotations

import json
import math
import os
import subprocess
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

DEFAULT_BIN = Path(__file__).resolve().parents[2] / "target" / "release" / "theo"

# Pricing per 1M tokens (input/output) — update as models change.
# Source: provider pricing pages as of 2026-04.
MODEL_PRICING: dict[str, tuple[float, float]] = {
    # (input_per_1M, output_per_1M)
    "gpt-4o": (2.50, 10.00),
    "gpt-4o-mini": (0.15, 0.60),
    "gpt-4.1": (2.00, 8.00),
    "gpt-4.1-mini": (0.40, 1.60),
    "gpt-4.1-nano": (0.10, 0.40),
    "claude-sonnet-4-5": (3.00, 15.00),
    "claude-opus-4": (15.00, 75.00),
    "claude-haiku-3-5": (0.80, 4.00),
    "o3": (2.00, 8.00),
    "o4-mini": (1.10, 4.40),
    "codex-mini": (1.50, 6.00),
    # Local/self-hosted models — zero cost
    "qwen": (0.0, 0.0),
    "deepseek": (0.0, 0.0),
    "llama": (0.0, 0.0),
}


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
    cost_usd: float = 0.0
    raw_json: Optional[dict] = None

    @classmethod
    def from_json(cls, data: dict, exit_code: int = 0) -> HeadlessResult:
        tokens = data.get("tokens") or {}
        tools = data.get("tools") or {}
        llm = data.get("llm") or {}
        files = data.get("files_edited") or []
        if isinstance(files, int):
            files = []

        tok_in = tokens.get("input", 0) or 0
        tok_out = tokens.get("output", 0) or 0
        model_name = data.get("model", "")

        return cls(
            success=data.get("success", False),
            summary=data.get("summary", ""),
            iterations=data.get("iterations", 0),
            duration_ms=data.get("duration_ms", 0),
            tokens_input=tok_in,
            tokens_output=tok_out,
            tokens_total=tokens.get("total", 0) or (tok_in + tok_out),
            tool_calls_total=tools.get("total", 0),
            tool_calls_success=tools.get("success", 0),
            llm_calls=llm.get("calls", 0),
            llm_retries=llm.get("retries", 0),
            files_edited=files if isinstance(files, list) else [],
            model=model_name,
            mode=data.get("mode", ""),
            provider=data.get("provider", ""),
            exit_code=exit_code,
            cost_usd=estimate_cost(model_name, tok_in, tok_out),
            raw_json=data,
        )

    @classmethod
    def from_error(cls, error: str, exit_code: int = -1) -> HeadlessResult:
        return cls(success=False, error=error, exit_code=exit_code)


@dataclass
class AggregatedResult:
    """Statistics across multiple runs of the same task."""

    task_id: str
    n_runs: int
    success_rate: float
    mean_iterations: float
    std_iterations: float
    mean_duration_ms: float
    mean_tokens_total: int
    mean_cost_usd: float
    total_cost_usd: float
    ci_95_lower: float  # 95% CI for success rate
    ci_95_upper: float
    runs: list[HeadlessResult] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Cost estimation
# ---------------------------------------------------------------------------


def estimate_cost(model: str, tokens_in: int, tokens_out: int) -> float:
    """Estimate USD cost from token counts and model name."""
    if not model or (tokens_in == 0 and tokens_out == 0):
        return 0.0

    model_lower = model.lower()
    for key, (price_in, price_out) in MODEL_PRICING.items():
        if key in model_lower:
            return (tokens_in * price_in + tokens_out * price_out) / 1_000_000
    return 0.0  # Unknown model — assume self-hosted


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
    temperature: Optional[float] = None,
    theo_bin: Optional[Path] = None,
    env_extra: Optional[dict[str, str]] = None,
    retries: int = 3,
    retry_wait: int = 30,
) -> HeadlessResult:
    """Run `theo --headless` and return parsed result.

    This is the ONLY way benchmark code should invoke the agent.

    Args:
        temperature: Fixed sampling temperature. Use 0.0 for deterministic
                     benchmarks. If None, uses the binary's default.
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
    # Determinism controls via env vars (the Rust binary reads these)
    if temperature is not None:
        env["THEO_TEMPERATURE"] = str(temperature)
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


# ---------------------------------------------------------------------------
# Multi-run with statistics
# ---------------------------------------------------------------------------


def run_headless_multi(
    prompt: str,
    repo: str | Path = ".",
    *,
    n_runs: int = 3,
    task_id: str = "",
    **kwargs,
) -> AggregatedResult:
    """Run the same task multiple times and aggregate statistics.

    Returns AggregatedResult with mean, std, and 95% confidence interval
    for success rate (Wilson score interval).
    """
    runs: list[HeadlessResult] = []
    for i in range(n_runs):
        result = run_headless(prompt, repo, **kwargs)
        runs.append(result)

    successes = sum(1 for r in runs if r.success)
    n = len(runs)
    rate = successes / n if n > 0 else 0.0

    iterations = [r.iterations for r in runs]
    mean_iter = sum(iterations) / n if n > 0 else 0.0
    std_iter = _std(iterations) if n > 1 else 0.0

    durations = [r.duration_ms for r in runs]
    mean_dur = sum(durations) / n if n > 0 else 0.0

    tokens = [r.tokens_total for r in runs]
    mean_tok = int(sum(tokens) / n) if n > 0 else 0

    costs = [r.cost_usd for r in runs]
    mean_cost = sum(costs) / n if n > 0 else 0.0
    total_cost = sum(costs)

    ci_lo, ci_hi = _wilson_ci(successes, n)

    return AggregatedResult(
        task_id=task_id,
        n_runs=n,
        success_rate=rate,
        mean_iterations=round(mean_iter, 1),
        std_iterations=round(std_iter, 1),
        mean_duration_ms=round(mean_dur, 0),
        mean_tokens_total=mean_tok,
        mean_cost_usd=round(mean_cost, 4),
        total_cost_usd=round(total_cost, 4),
        ci_95_lower=round(ci_lo, 3),
        ci_95_upper=round(ci_hi, 3),
        runs=runs,
    )


# ---------------------------------------------------------------------------
# Statistics helpers
# ---------------------------------------------------------------------------


def _std(values: list[float | int]) -> float:
    """Sample standard deviation."""
    n = len(values)
    if n < 2:
        return 0.0
    mean = sum(values) / n
    variance = sum((x - mean) ** 2 for x in values) / (n - 1)
    return math.sqrt(variance)


def _wilson_ci(successes: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """Wilson score interval for binomial proportion (95% CI).

    More accurate than normal approximation for small n.
    """
    if n == 0:
        return 0.0, 0.0
    p = successes / n
    denom = 1 + z * z / n
    centre = p + z * z / (2 * n)
    spread = z * math.sqrt((p * (1 - p) + z * z / (4 * n)) / n)
    lower = max(0.0, (centre - spread) / denom)
    upper = min(1.0, (centre + spread) / denom)
    return lower, upper


# ---------------------------------------------------------------------------
# JSON parsing
# ---------------------------------------------------------------------------


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
    temperature: Optional[float] = None,
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
            temperature=temperature,
            theo_bin=theo_bin,
        )
        if result.success:
            return True, result.summary, ""
        return False, "", result.error or result.summary or "Agent did not converge"

    return agent_fn
