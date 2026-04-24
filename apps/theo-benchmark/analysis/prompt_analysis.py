"""
Prompt Analysis — benchmark-sota-metrics-plan.

Analyzes token efficiency, instruction adherence (agent claim vs ground truth),
and output density across benchmark results.
"""

from __future__ import annotations

from .stats_utils import mean, safe_div


def analyze_prompts(
    results: list[dict],
    check_results: list[dict] | None = None,
) -> dict:
    """Analyze prompt-level metrics across benchmark results.

    Args:
        results: list of dicts with fields tokens_input, tokens_output,
                 reasoning_tokens, success (agent's self-reported claim).
        check_results: optional list of dicts with {task_id: str,
                       check_passed: bool} — the ground truth from external
                       verification (e.g. test suite, grader).

    Returns:
        Plain dict with token_ratio, instruction_adherence, and efficiency.
    """
    if not results:
        return _empty_result()

    # --- Token ratio ---
    inputs = [float(r.get("tokens_input", 0) or 0) for r in results]
    outputs = [float(r.get("tokens_output", 0) or 0) for r in results]
    reasoning = [float(r.get("reasoning_tokens", 0) or 0) for r in results]

    avg_in = mean(inputs)
    avg_out = mean(outputs)
    avg_reason = mean(reasoning)

    token_ratio = {
        "avg_input_tokens": round(avg_in, 2),
        "avg_output_tokens": round(avg_out, 2),
        "avg_input_output_ratio": round(safe_div(avg_in, avg_out), 4),
        "avg_reasoning_tokens": round(avg_reason, 2),
        "reasoning_pct_of_output": round(
            safe_div(avg_reason, avg_out) * 100, 2
        ),
    }

    # --- Instruction adherence ---
    instruction_adherence = _compute_adherence(results, check_results)

    # --- Efficiency ---
    efficiency = _compute_efficiency(results)

    return {
        "token_ratio": token_ratio,
        "instruction_adherence": instruction_adherence,
        "efficiency": efficiency,
    }


def _compute_adherence(
    results: list[dict],
    check_results: list[dict] | None,
) -> dict:
    """Compare agent self-reported success vs external ground truth."""
    if not check_results:
        return {
            "tasks_with_check": 0,
            "false_positive_rate": 0.0,
            "false_negative_rate": 0.0,
            "agreement_rate": 0.0,
        }

    # Index check results by task_id
    checks_by_id: dict[str, bool] = {}
    for cr in check_results:
        tid = cr.get("task_id", "")
        if tid:
            checks_by_id[tid] = bool(cr.get("check_passed", False))

    # Match results that have a corresponding check
    matched = 0
    false_positives = 0
    false_negatives = 0
    agreements = 0

    for i, r in enumerate(results):
        task_id = r.get("task_id", str(i))
        if task_id not in checks_by_id:
            continue

        matched += 1
        agent_success = bool(r.get("success", False))
        check_passed = checks_by_id[task_id]

        if agent_success == check_passed:
            agreements += 1
        elif agent_success and not check_passed:
            false_positives += 1
        elif not agent_success and check_passed:
            false_negatives += 1

    return {
        "tasks_with_check": matched,
        "false_positive_rate": round(safe_div(false_positives, matched), 4),
        "false_negative_rate": round(safe_div(false_negatives, matched), 4),
        "agreement_rate": round(safe_div(agreements, matched), 4),
    }


def _compute_efficiency(results: list[dict]) -> dict:
    """Compute token efficiency split by pass/fail."""
    pass_tokens: list[float] = []
    fail_tokens: list[float] = []

    for r in results:
        total = float(r.get("tokens_input", 0) or 0) + float(
            r.get("tokens_output", 0) or 0
        )
        if bool(r.get("success", False)):
            pass_tokens.append(total)
        else:
            fail_tokens.append(total)

    avg_pass = mean(pass_tokens)
    avg_fail = mean(fail_tokens)

    # Output density: avg output tokens / avg total tokens
    total_all = [
        float(r.get("tokens_input", 0) or 0)
        + float(r.get("tokens_output", 0) or 0)
        for r in results
    ]
    output_all = [float(r.get("tokens_output", 0) or 0) for r in results]
    output_density = safe_div(mean(output_all), mean(total_all))

    return {
        "tokens_per_task_pass": round(avg_pass, 2),
        "tokens_per_task_fail": round(avg_fail, 2),
        "output_density": round(output_density, 4),
    }


def _empty_result() -> dict:
    return {
        "token_ratio": {
            "avg_input_tokens": 0.0,
            "avg_output_tokens": 0.0,
            "avg_input_output_ratio": 0.0,
            "avg_reasoning_tokens": 0.0,
            "reasoning_pct_of_output": 0.0,
        },
        "instruction_adherence": {
            "tasks_with_check": 0,
            "false_positive_rate": 0.0,
            "false_negative_rate": 0.0,
            "agreement_rate": 0.0,
        },
        "efficiency": {
            "tokens_per_task_pass": 0.0,
            "tokens_per_task_fail": 0.0,
            "output_density": 0.0,
        },
    }
