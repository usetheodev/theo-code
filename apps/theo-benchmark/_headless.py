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
    """Parsed output from `theo --headless`.

    v4 fields: when the JSON contains a `report` object (RunReport from
    the Rust runtime), all extended metrics are populated. For v3 JSON
    (no `report` field), extended fields use safe defaults (0 / 0.0 / []).
    """

    # --- Core fields (v1-v3) ---
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

    # --- Extended token metrics (v4 — from report.token_metrics) ---
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    reasoning_tokens: int = 0
    cache_hit_rate: float = 0.0
    tokens_per_successful_edit: float = 0.0

    # --- Loop metrics (v4 — from report.loop_metrics) ---
    convergence_rate: float = 0.0
    budget_utilization_iterations_pct: float = 0.0
    budget_utilization_tokens_pct: float = 0.0
    budget_utilization_time_pct: float = 0.0
    evolution_attempts: int = 0
    evolution_success: bool = False
    done_blocked_count: int = 0
    phase_distribution: dict = field(default_factory=dict)

    # --- Tool breakdown (v4 — from report.tool_breakdown) ---
    tool_breakdown: list[dict] = field(default_factory=list)

    # --- Context health (v4 — from report.context_health) ---
    context_avg_size_tokens: float = 0.0
    context_max_size_tokens: int = 0
    context_growth_rate: float = 0.0
    context_compaction_count: int = 0
    context_compaction_savings_ratio: float = 0.0
    context_refetch_rate: float = 0.0
    context_action_repetition_rate: float = 0.0
    context_usefulness_avg: float = 0.0

    # --- Memory metrics (v4 — from report.memory_metrics) ---
    memory_episodes_injected: int = 0
    memory_episodes_created: int = 0
    memory_hypotheses_formed: int = 0
    memory_hypotheses_invalidated: int = 0
    memory_hypotheses_active: int = 0
    memory_constraints_learned: int = 0
    memory_failure_fingerprints_new: int = 0
    memory_failure_fingerprints_recurrent: int = 0

    # --- Subagent metrics (v4 — from report.subagent_metrics) ---
    subagent_spawned: int = 0
    subagent_succeeded: int = 0
    subagent_failed: int = 0
    subagent_avg_duration_ms: float = 0.0
    subagent_success_rate: float = 0.0

    # --- Error taxonomy (v4 — from report.error_taxonomy) ---
    error_total: int = 0
    error_network: int = 0
    error_llm: int = 0
    error_tool: int = 0
    error_sandbox: int = 0
    error_budget: int = 0
    error_validation: int = 0

    # --- Derived/surrogate metrics (v4 — from report.surrogate_metrics) ---
    doom_loop_frequency: float = 0.0
    llm_efficiency: float = 0.0
    context_waste_ratio: float = 0.0
    hypothesis_churn_rate: float = 0.0
    time_to_first_tool_ms: float = 0.0

    # --- Integrity (v4 — from report.integrity) ---
    trajectory_complete: bool = True
    trajectory_confidence: float = 1.0

    # --- Error class (v3+) ---
    error_class: str = ""

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

        result = cls(
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
            error_class=data.get("error_class", ""),
        )

        # v4: parse RunReport from "report" field
        report = data.get("report")
        if report and isinstance(report, dict):
            result._parse_report(report)

        return result

    def _parse_report(self, report: dict) -> None:
        """Extract all RunReport sections into flat fields."""
        # Token metrics
        tm = report.get("token_metrics") or {}
        self.cache_read_tokens = tm.get("cache_read_tokens", 0)
        self.cache_write_tokens = tm.get("cache_write_tokens", 0)
        self.reasoning_tokens = tm.get("reasoning_tokens", 0)
        self.cache_hit_rate = tm.get("cache_hit_rate", 0.0)
        self.tokens_per_successful_edit = tm.get("tokens_per_successful_edit", 0.0)

        # Loop metrics
        lm = report.get("loop_metrics") or {}
        self.convergence_rate = lm.get("convergence_rate", 0.0)
        self.done_blocked_count = lm.get("done_blocked_count", 0)
        self.evolution_attempts = lm.get("evolution_attempts", 0)
        self.evolution_success = lm.get("evolution_success", False)
        self.phase_distribution = lm.get("phase_distribution") or {}
        bu = lm.get("budget_utilization") or {}
        self.budget_utilization_iterations_pct = bu.get("iterations_pct", 0.0)
        self.budget_utilization_tokens_pct = bu.get("tokens_pct", 0.0)
        self.budget_utilization_time_pct = bu.get("time_pct", 0.0)

        # Tool breakdown
        self.tool_breakdown = report.get("tool_breakdown") or []

        # Context health
        ch = report.get("context_health") or {}
        self.context_avg_size_tokens = ch.get("avg_context_size_tokens", 0.0)
        self.context_max_size_tokens = ch.get("max_context_size_tokens", 0)
        self.context_growth_rate = ch.get("context_growth_rate", 0.0)
        self.context_compaction_count = ch.get("compaction_count", 0)
        self.context_compaction_savings_ratio = ch.get("compaction_savings_ratio", 0.0)
        self.context_refetch_rate = ch.get("refetch_rate", 0.0)
        self.context_action_repetition_rate = ch.get("action_repetition_rate", 0.0)
        self.context_usefulness_avg = ch.get("usefulness_avg", 0.0)

        # Memory metrics
        mm = report.get("memory_metrics") or {}
        self.memory_episodes_injected = mm.get("episodes_injected", 0)
        self.memory_episodes_created = mm.get("episodes_created", 0)
        self.memory_hypotheses_formed = mm.get("hypotheses_formed", 0)
        self.memory_hypotheses_invalidated = mm.get("hypotheses_invalidated", 0)
        self.memory_hypotheses_active = mm.get("hypotheses_active", 0)
        self.memory_constraints_learned = mm.get("constraints_learned", 0)
        self.memory_failure_fingerprints_new = mm.get("failure_fingerprints_new", 0)
        self.memory_failure_fingerprints_recurrent = mm.get("failure_fingerprints_recurrent", 0)

        # Subagent metrics
        sm = report.get("subagent_metrics") or {}
        self.subagent_spawned = sm.get("spawned", 0)
        self.subagent_succeeded = sm.get("succeeded", 0)
        self.subagent_failed = sm.get("failed", 0)
        self.subagent_avg_duration_ms = sm.get("avg_duration_ms", 0.0)
        self.subagent_success_rate = sm.get("success_rate", 0.0)

        # Error taxonomy
        et = report.get("error_taxonomy") or {}
        self.error_total = et.get("total_errors", 0)
        self.error_network = et.get("network_errors", 0)
        self.error_llm = et.get("llm_errors", 0)
        self.error_tool = et.get("tool_errors", 0)
        self.error_sandbox = et.get("sandbox_errors", 0)
        self.error_budget = et.get("budget_errors", 0)
        self.error_validation = et.get("validation_errors", 0)

        # Derived/surrogate metrics
        dm = report.get("surrogate_metrics") or {}
        self.doom_loop_frequency = _surrogate_value(dm, "doom_loop_frequency")
        self.llm_efficiency = _surrogate_value(dm, "llm_efficiency")
        self.context_waste_ratio = _surrogate_value(dm, "context_waste_ratio")
        self.hypothesis_churn_rate = _surrogate_value(dm, "hypothesis_churn_rate")
        self.time_to_first_tool_ms = _surrogate_value(dm, "time_to_first_tool_ms")

        # Integrity
        integrity = report.get("integrity") or {}
        self.trajectory_complete = integrity.get("complete", True)
        self.trajectory_confidence = integrity.get("confidence", 1.0)

    @classmethod
    def from_error(cls, error: str, exit_code: int = -1) -> HeadlessResult:
        return cls(success=False, error=error, exit_code=exit_code)


def _surrogate_value(parent: dict, key: str) -> float:
    """Extract .value from a SurrogateMetric dict, defaulting to 0.0."""
    entry = parent.get(key)
    if isinstance(entry, dict):
        return entry.get("value", 0.0)
    return 0.0


@dataclass
class AggregatedResult:
    """Statistics across multiple runs of the same task.

    v4 fields aggregate the extended HeadlessResult metrics with mean values.
    """

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
    # --- v4 extended aggregates ---
    mean_cache_hit_rate: float = 0.0
    mean_convergence_rate: float = 0.0
    mean_budget_utilization_pct: float = 0.0
    mean_context_max_size: float = 0.0
    mean_context_growth_rate: float = 0.0
    mean_doom_loop_frequency: float = 0.0
    mean_llm_efficiency: float = 0.0
    mean_context_waste_ratio: float = 0.0
    mean_time_to_first_tool_ms: float = 0.0
    total_subagent_spawned: int = 0
    total_errors: int = 0
    tool_breakdown_aggregate: dict = field(default_factory=dict)


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
    # Pass temperature as CLI flag (highest precedence, explicit > env var)
    if temperature is not None:
        cmd.extend(["--temperature", str(temperature)])
    cmd.append(prompt)

    env = os.environ.copy()
    # Also set env var as fallback (for compatibility)
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

    # v4 extended aggregates
    def _mean_field(attr: str) -> float:
        vals = [getattr(r, attr, 0.0) for r in runs]
        return sum(vals) / n if n > 0 else 0.0

    # Tool breakdown: merge per-tool stats across runs
    tool_agg: dict[str, dict] = {}
    for r in runs:
        for tb in r.tool_breakdown:
            name = tb.get("tool_name", "unknown")
            if name not in tool_agg:
                tool_agg[name] = {"call_count": 0, "success_count": 0, "failure_count": 0, "latency_sum": 0.0, "latency_n": 0}
            tool_agg[name]["call_count"] += tb.get("call_count", 0)
            tool_agg[name]["success_count"] += tb.get("success_count", 0)
            tool_agg[name]["failure_count"] += tb.get("failure_count", 0)
            avg_lat = tb.get("avg_latency_ms", 0.0)
            cc = tb.get("call_count", 0)
            if avg_lat > 0 and cc > 0:
                tool_agg[name]["latency_sum"] += avg_lat * cc
                tool_agg[name]["latency_n"] += cc
    # Finalize tool aggregates
    for ta in tool_agg.values():
        total = ta["call_count"]
        ta["success_rate"] = ta["success_count"] / total if total > 0 else 0.0
        ta["avg_latency_ms"] = ta["latency_sum"] / ta["latency_n"] if ta["latency_n"] > 0 else 0.0
        del ta["latency_sum"]
        del ta["latency_n"]

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
        mean_cache_hit_rate=round(_mean_field("cache_hit_rate"), 4),
        mean_convergence_rate=round(_mean_field("convergence_rate"), 4),
        mean_budget_utilization_pct=round(_mean_field("budget_utilization_iterations_pct"), 4),
        mean_context_max_size=round(_mean_field("context_max_size_tokens"), 1),
        mean_context_growth_rate=round(_mean_field("context_growth_rate"), 4),
        mean_doom_loop_frequency=round(_mean_field("doom_loop_frequency"), 4),
        mean_llm_efficiency=round(_mean_field("llm_efficiency"), 4),
        mean_context_waste_ratio=round(_mean_field("context_waste_ratio"), 4),
        mean_time_to_first_tool_ms=round(_mean_field("time_to_first_tool_ms"), 1),
        total_subagent_spawned=sum(r.subagent_spawned for r in runs),
        total_errors=sum(r.error_total for r in runs),
        tool_breakdown_aggregate=tool_agg,
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
