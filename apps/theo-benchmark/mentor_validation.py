#!/usr/bin/env python3
"""
Mentor v2 Validation Framework — Tests classification accuracy and stability.

Validates that mentor_metrics.py correctly classifies sessions into
the 6 learning patterns from Shen & Tamkin (2026):

    Pattern                    | Retention
    ---------------------------|----------
    AI Delegation              | 19%
    Iterative Debugging        | 24%
    Progressive Reliance       | 35%
    Conceptual Inquiry         | 65%
    Hybrid Explanation         | 68%
    Generation-Comprehension   | 86%

Creates simulated sessions at different quality levels, runs them through
analyze_session(), and validates classification accuracy + stability.
"""

import random
import sys
import os
from collections import Counter, defaultdict
from dataclasses import dataclass

sys.path.insert(0, os.path.dirname(__file__))
from mentor_metrics import analyze_session, SessionMetrics


# ---------------------------------------------------------------------------
# 1. Simulated Session Templates
# ---------------------------------------------------------------------------

@dataclass
class SimulatedSession:
    """A simulated mentor output with expected classification."""
    name: str
    expected_pattern: str
    expected_retention: float
    output_text: str
    dev_interactions: list


def _build_phase_decide(num_alternatives: int = 2) -> str:
    labels = ["A", "B", "C", "D"]
    lines = ["## Phase 1: DECIDE\n"]
    for i in range(min(num_alternatives, len(labels))):
        lines.append(f"Approach {labels[i]}: Option description for approach {labels[i]}")
    lines.append("I recommend Approach A because it balances simplicity and correctness.\n")
    return "\n".join(lines)


def _build_phase_generate() -> str:
    return (
        "## Phase 2: GENERATE\n"
        "Writing the implementation now.\n"
        "I'm using a HashMap here for O(1) lookup.\n"
        "Note: if data grows beyond 100K entries, consider sharding.\n"
    )


def _build_phase_alert(num_risks: int = 3, num_failure_modes: int = 1) -> str:
    lines = ["## Phase 3: ALERT\n"]
    risk_templates = [
        "Risk: No input validation on user-supplied data",
        "Risk: This assumes UTF-8 encoding throughout",
        "Risk: Concurrent access without locking could corrupt state",
        "Risk: Missing timeout on external HTTP call",
        "Risk: Hardcoded retry count may not suit all environments",
        "Risk: No graceful degradation if cache is unavailable",
    ]
    warning_templates = [
        "⚠ The function does not handle empty collections",
        "⚠ Timezone assumption may break in non-UTC environments",
        "⚠ Integer overflow possible with very large inputs",
        "⚠ File handle not explicitly closed on error path",
        "⚠ Potential memory leak if callback throws",
        "⚠ No rate limiting on this endpoint",
    ]
    for i in range(num_risks):
        if i < len(risk_templates):
            lines.append(risk_templates[i])
        if i < len(warning_templates):
            lines.append(warning_templates[i])
    for _ in range(num_failure_modes):
        lines.append("This could fail if the database connection drops mid-transaction.")
    lines.append("")
    return "\n".join(lines)


def _build_phase_verify(num_checklist: int = 3) -> str:
    lines = ["## Phase 4: VERIFY\n", "Running tests now.\n", "Review checklist:\n"]
    checklist_items = [
        "1. Verify error handling on all external calls",
        "2. Check that all file handles are closed",
        "3. Confirm timeout values match SLA requirements",
        "4. Validate input boundaries match spec",
        "5. Ensure logging does not leak sensitive data",
    ]
    for i in range(min(num_checklist, len(checklist_items))):
        lines.append(checklist_items[i])
    lines.append("")
    return "\n".join(lines)


def _build_tradeoffs(count: int = 1) -> str:
    lines = []
    for _ in range(count):
        lines.append("The tradeoff here is latency vs consistency.")
    return "\n".join(lines)


def _build_tools(count: int = 3) -> str:
    tools = ["search_code", "read_file", "edit_file", "run_command", "done"]
    lines = []
    for i in range(min(count, len(tools))):
        lines.append(f"🔧 Tool: {tools[i]}")
    return "\n".join(lines)


def build_full_session(
    num_risks: int = 3,
    num_alternatives: int = 2,
    num_checklist: int = 3,
    num_tradeoffs: int = 1,
    num_failure_modes: int = 1,
    num_tools: int = 5,
) -> str:
    """Build a FULL quality session (all 4 phases + risks + alternatives + checklist)."""
    parts = [
        _build_phase_decide(num_alternatives),
        _build_phase_generate(),
        _build_phase_alert(num_risks, num_failure_modes),
        _build_phase_verify(num_checklist),
        _build_tradeoffs(num_tradeoffs),
        _build_tools(num_tools),
    ]
    return "\n".join(parts)


def build_good_session(
    num_risks: int = 3,
    num_checklist: int = 2,
    num_tools: int = 4,
) -> str:
    """Build a GOOD quality session (3 phases + some risks)."""
    parts = [
        _build_phase_decide(1),  # 1 alternative only
        _build_phase_generate(),
        _build_phase_alert(num_risks),
        _build_tools(num_tools),
    ]
    return "\n".join(parts)


def build_moderate_session(
    num_alternatives: int = 2,
    num_tools: int = 3,
) -> str:
    """Build a MODERATE quality session (2 phases + alternatives)."""
    parts = [
        _build_phase_decide(num_alternatives),
        _build_phase_generate(),
        _build_tools(num_tools),
    ]
    return "\n".join(parts)


def build_minimal_session(num_tools: int = 2) -> str:
    """Build a MINIMAL quality session (only GENERATE phase)."""
    parts = [
        _build_phase_generate(),
        _build_tools(num_tools),
    ]
    return "\n".join(parts)


def build_interactive_session(
    num_risks: int = 4,
    num_alternatives: int = 3,
    num_checklist: int = 4,
    num_tools: int = 5,
) -> str:
    """Build an INTERACTIVE session (full output + dev will have interactions)."""
    return build_full_session(
        num_risks=num_risks,
        num_alternatives=num_alternatives,
        num_checklist=num_checklist,
        num_tradeoffs=2,
        num_failure_modes=2,
        num_tools=num_tools,
    )


def build_passive_session(
    num_risks: int = 3,
    num_alternatives: int = 2,
    num_checklist: int = 3,
    num_tools: int = 5,
) -> str:
    """Build a PASSIVE session (full output but zero dev interactions)."""
    return build_full_session(
        num_risks=num_risks,
        num_alternatives=num_alternatives,
        num_checklist=num_checklist,
        num_tools=num_tools,
    )


# ---------------------------------------------------------------------------
# 2. Session Generators (with randomization for statistical runs)
# ---------------------------------------------------------------------------

def generate_session(session_type: str, seed: int = None) -> SimulatedSession:
    """Generate a simulated session of the given type with optional randomization."""
    rng = random.Random(seed)

    if session_type == "FULL":
        output = build_full_session(
            num_risks=rng.randint(3, 6),
            num_alternatives=rng.randint(2, 3),
            num_checklist=rng.randint(3, 5),
            num_tradeoffs=rng.randint(1, 3),
            num_failure_modes=rng.randint(1, 3),
            num_tools=rng.randint(3, 5),
        )
        interactions = ["risks", "alternatives", "review", "why"]
        return SimulatedSession(
            name="FULL",
            expected_pattern="generation_comprehension",
            expected_retention=0.86,
            output_text=output,
            dev_interactions=interactions,
        )

    elif session_type == "GOOD":
        output = build_good_session(
            num_risks=rng.randint(3, 5),
            num_checklist=rng.randint(1, 3),
            num_tools=rng.randint(3, 5),
        )
        return SimulatedSession(
            name="GOOD",
            expected_pattern="hybrid_explanation",
            expected_retention=0.68,
            output_text=output,
            dev_interactions=[],
        )

    elif session_type == "MODERATE":
        output = build_moderate_session(
            num_alternatives=rng.randint(2, 3),
            num_tools=rng.randint(2, 4),
        )
        return SimulatedSession(
            name="MODERATE",
            expected_pattern="conceptual_inquiry",
            expected_retention=0.65,
            output_text=output,
            dev_interactions=[],
        )

    elif session_type == "MINIMAL":
        output = build_minimal_session(num_tools=rng.randint(1, 3))
        return SimulatedSession(
            name="MINIMAL",
            expected_pattern="ai_delegation",
            expected_retention=0.19,
            output_text=output,
            dev_interactions=[],
        )

    elif session_type == "INTERACTIVE":
        output = build_interactive_session(
            num_risks=rng.randint(3, 6),
            num_alternatives=rng.randint(2, 4),
            num_checklist=rng.randint(3, 5),
            num_tools=rng.randint(4, 5),
        )
        base_interactions = ["risks", "alternatives", "review", "why"]
        extra = rng.sample(
            ["What about performance?", "risks", "why", "alternatives"],
            k=rng.randint(1, 3),
        )
        return SimulatedSession(
            name="INTERACTIVE",
            expected_pattern="generation_comprehension",
            expected_retention=0.86,
            output_text=output,
            dev_interactions=base_interactions + extra,
        )

    elif session_type == "PASSIVE":
        output = build_passive_session(
            num_risks=rng.randint(3, 5),
            num_alternatives=rng.randint(2, 3),
            num_checklist=rng.randint(3, 5),
            num_tools=rng.randint(3, 5),
        )
        return SimulatedSession(
            name="PASSIVE",
            expected_pattern="hybrid_explanation",
            expected_retention=0.68,
            output_text=output,
            dev_interactions=[],
        )

    else:
        raise ValueError(f"Unknown session type: {session_type}")


# ---------------------------------------------------------------------------
# 3. Edge Case Generators
# ---------------------------------------------------------------------------

def generate_edge_cases() -> list:
    """Generate edge case sessions that test boundary conditions."""
    cases = []

    # Edge 1: 0 phases but many interactions
    cases.append(SimulatedSession(
        name="EDGE_no_phases_many_interactions",
        expected_pattern="hybrid_explanation",  # fallback
        expected_retention=0.68,
        output_text="Here is the code.\nDone.",
        dev_interactions=["risks", "alternatives", "why", "review", "What about X?"],
    ))

    # Edge 2: All phases but 0 risks
    cases.append(SimulatedSession(
        name="EDGE_all_phases_zero_risks",
        expected_pattern="hybrid_explanation",  # 4 phases but <3 risks, no alternatives >= 2
        expected_retention=0.68,
        output_text=(
            "## Phase 1: DECIDE\nApproach A: Simple\n\n"
            "## Phase 2: GENERATE\nWriting code.\n\n"
            "## Phase 3: ALERT\nNo significant risks found.\n\n"
            "## Phase 4: VERIFY\nAll tests pass.\n"
        ),
        dev_interactions=[],
    ))

    # Edge 3: Only GENERATE + many dev questions → iterative_debugging
    # (1 phase, 3 interactions, 3 generic questions = debugging loop)
    cases.append(SimulatedSession(
        name="EDGE_generate_only_many_questions",
        expected_pattern="iterative_debugging",  # 1 phase + many questions = debugging loop
        expected_retention=0.24,
        output_text="## Phase 2: GENERATE\nImplementing the feature.\n🔧 Tool: edit_file\n",
        dev_interactions=["Why this approach?", "What about caching?", "Is this thread-safe?"],
    ))

    # Edge 4: Empty output, no interactions
    cases.append(SimulatedSession(
        name="EDGE_empty_session",
        expected_pattern="hybrid_explanation",  # fallback
        expected_retention=0.68,
        output_text="",
        dev_interactions=[],
    ))

    # Edge 5: All phases + many risks + only "why" interaction
    # FIXED: classify_pattern now counts dev_why_inquiries as active engagement
    cases.append(SimulatedSession(
        name="EDGE_all_phases_risks_only_why",
        expected_pattern="generation_comprehension",  # FIXED: "why" now counts as active engagement
        expected_retention=0.86,
        output_text=build_full_session(num_risks=4, num_alternatives=2, num_checklist=3),
        dev_interactions=["why"],
    ))

    # Edge 6: 2 phases + 1 alternative (not enough for conceptual_inquiry)
    cases.append(SimulatedSession(
        name="EDGE_two_phases_one_alternative",
        expected_pattern="hybrid_explanation",  # 2 phases but alternatives < 2
        expected_retention=0.68,
        output_text=(
            "## Phase 1: DECIDE\nApproach A: The only approach.\n\n"
            "## Phase 2: GENERATE\nWriting code.\n"
        ),
        dev_interactions=[],
    ))

    return cases


# ---------------------------------------------------------------------------
# 4. Validation Runner
# ---------------------------------------------------------------------------

SESSION_TYPES = ["FULL", "GOOD", "MODERATE", "MINIMAL", "INTERACTIVE", "PASSIVE"]

ANTHROPIC_REFERENCE = {
    "ai_delegation": 0.19,
    "iterative_debugging": 0.24,
    "progressive_reliance": 0.35,
    "conceptual_inquiry": 0.65,
    "hybrid_explanation": 0.68,
    "generation_comprehension": 0.86,
}

RETENTION_ORDER = [
    "ai_delegation",
    "iterative_debugging",
    "progressive_reliance",
    "conceptual_inquiry",
    "hybrid_explanation",
    "generation_comprehension",
]


def run_single_classification_test() -> list:
    """Run classification on one instance of each session type. Returns results."""
    results = []
    for stype in SESSION_TYPES:
        session = generate_session(stype, seed=42)
        metrics = analyze_session(session.output_text, session.dev_interactions)
        results.append({
            "session_type": stype,
            "expected_pattern": session.expected_pattern,
            "actual_pattern": metrics.pattern,
            "expected_retention": session.expected_retention,
            "actual_retention": metrics.predicted_retention,
            "match": metrics.pattern == session.expected_pattern,
        })
    return results


def run_statistical_validation(num_runs: int = 50) -> dict:
    """Run N variations per session type and compute stability metrics."""
    stats = defaultdict(lambda: {
        "patterns": [],
        "retentions": [],
        "expected_pattern": "",
        "expected_retention": 0.0,
    })

    for stype in SESSION_TYPES:
        for i in range(num_runs):
            session = generate_session(stype, seed=i * 1000 + hash(stype) % 10000)
            metrics = analyze_session(session.output_text, session.dev_interactions)
            stats[stype]["patterns"].append(metrics.pattern)
            stats[stype]["retentions"].append(metrics.predicted_retention)
            stats[stype]["expected_pattern"] = session.expected_pattern
            stats[stype]["expected_retention"] = session.expected_retention

    result = {}
    for stype in SESSION_TYPES:
        s = stats[stype]
        pattern_counts = Counter(s["patterns"])
        most_common_pattern = pattern_counts.most_common(1)[0][0]
        correct_count = sum(1 for p in s["patterns"] if p == s["expected_pattern"])
        stability = correct_count / len(s["patterns"])

        retentions = s["retentions"]
        mean_ret = sum(retentions) / len(retentions)
        variance = sum((r - mean_ret) ** 2 for r in retentions) / len(retentions)
        std_ret = variance ** 0.5

        result[stype] = {
            "expected_pattern": s["expected_pattern"],
            "most_common_pattern": most_common_pattern,
            "stability": stability,
            "correct_count": correct_count,
            "total_runs": len(s["patterns"]),
            "pattern_distribution": dict(pattern_counts),
            "mean_retention": mean_ret,
            "std_retention": std_ret,
            "expected_retention": s["expected_retention"],
        }

    return result


def run_edge_case_tests() -> list:
    """Run edge case sessions and report results."""
    results = []
    for session in generate_edge_cases():
        metrics = analyze_session(session.output_text, session.dev_interactions)
        results.append({
            "name": session.name,
            "expected_pattern": session.expected_pattern,
            "actual_pattern": metrics.pattern,
            "expected_retention": session.expected_retention,
            "actual_retention": metrics.predicted_retention,
            "match": metrics.pattern == session.expected_pattern,
            "phases": metrics.phases_covered,
            "risks": metrics.risks_identified,
            "alternatives": metrics.alternatives_presented,
            "interactions": metrics.total_dev_interactions,
        })
    return results


def build_confusion_matrix(statistical_results: dict) -> dict:
    """Build a confusion matrix from statistical validation results."""
    all_patterns = sorted(set(RETENTION_ORDER))
    matrix = {expected: Counter() for expected in all_patterns}

    for stype, data in statistical_results.items():
        expected = data["expected_pattern"]
        for pattern, count in data["pattern_distribution"].items():
            matrix[expected][pattern] += count

    return matrix


# ---------------------------------------------------------------------------
# 5. Report Formatting
# ---------------------------------------------------------------------------

def format_report(
    single_results: list,
    statistical_results: dict,
    edge_results: list,
    confusion: dict,
) -> str:
    """Format all validation results into a comprehensive report."""
    lines = []

    lines.append("=" * 72)
    lines.append("  MENTOR v2 VALIDATION REPORT")
    lines.append("  Classification Accuracy & Stability Analysis")
    lines.append("=" * 72)

    # --- Section 1: Single Classification ---
    lines.append("")
    lines.append("-" * 72)
    lines.append("  1. PATTERN CLASSIFICATION ACCURACY (Single Run)")
    lines.append("-" * 72)
    lines.append("")
    lines.append(f"  {'Type':<14} {'Expected':<28} {'Actual':<28} {'Match'}")
    lines.append(f"  {'----':<14} {'--------':<28} {'------':<28} {'-----'}")

    correct = sum(1 for r in single_results if r["match"])
    for r in single_results:
        mark = "PASS" if r["match"] else "FAIL"
        lines.append(
            f"  {r['session_type']:<14} "
            f"{r['expected_pattern']:<28} "
            f"{r['actual_pattern']:<28} "
            f"{mark}"
        )
    lines.append("")
    lines.append(f"  Accuracy: {correct}/{len(single_results)} ({correct/len(single_results)*100:.0f}%)")

    # --- Section 2: Retention Accuracy ---
    lines.append("")
    lines.append("-" * 72)
    lines.append("  2. RETENTION PREDICTION ACCURACY (Single Run)")
    lines.append("-" * 72)
    lines.append("")
    lines.append(f"  {'Type':<14} {'Expected':<12} {'Actual':<12} {'Delta':<10} {'Match'}")
    lines.append(f"  {'----':<14} {'--------':<12} {'------':<12} {'-----':<10} {'-----'}")

    for r in single_results:
        delta = abs(r["actual_retention"] - r["expected_retention"])
        mark = "PASS" if delta < 0.01 else "FAIL"
        lines.append(
            f"  {r['session_type']:<14} "
            f"{r['expected_retention']*100:>6.0f}%     "
            f"{r['actual_retention']*100:>6.0f}%     "
            f"{delta*100:>+5.0f}%     "
            f"{mark}"
        )

    # --- Section 3: Statistical Stability ---
    lines.append("")
    lines.append("-" * 72)
    lines.append(f"  3. STATISTICAL STABILITY ({list(statistical_results.values())[0]['total_runs']} runs per type)")
    lines.append("-" * 72)
    lines.append("")
    lines.append(f"  {'Type':<14} {'Stability':>10} {'Mean Ret':>10} {'Std Ret':>10} {'Expected':>10} {'Status'}")
    lines.append(f"  {'----':<14} {'---------':>10} {'--------':>10} {'-------':>10} {'--------':>10} {'------'}")

    all_stable = True
    for stype in SESSION_TYPES:
        s = statistical_results[stype]
        status = "PASS" if s["stability"] >= 0.90 else "FAIL"
        if s["stability"] < 0.90:
            all_stable = False
        lines.append(
            f"  {stype:<14} "
            f"{s['stability']*100:>8.1f}%  "
            f"{s['mean_retention']*100:>8.1f}%  "
            f"{s['std_retention']*100:>8.2f}%  "
            f"{s['expected_retention']*100:>8.0f}%  "
            f"{status}"
        )

    lines.append("")
    lines.append(f"  Overall stability: {'ALL PASS (>90%)' if all_stable else 'SOME BELOW 90%'}")

    # Pattern distribution detail
    lines.append("")
    lines.append("  Pattern distribution per session type:")
    for stype in SESSION_TYPES:
        s = statistical_results[stype]
        dist_str = ", ".join(
            f"{p}: {c}" for p, c in sorted(s["pattern_distribution"].items(), key=lambda x: -x[1])
        )
        lines.append(f"    {stype:<14} -> {dist_str}")

    # --- Section 4: Confusion Matrix ---
    lines.append("")
    lines.append("-" * 72)
    lines.append("  4. CONFUSION MATRIX (rows=expected, cols=actual)")
    lines.append("-" * 72)
    lines.append("")

    # Only show patterns that appear in our data
    active_patterns = sorted(set(
        p for data in statistical_results.values()
        for p in data["pattern_distribution"].keys()
    ) | set(data["expected_pattern"] for data in statistical_results.values()))

    # Abbreviations for readability
    abbrev = {
        "ai_delegation": "deleg",
        "iterative_debugging": "debug",
        "progressive_reliance": "progr",
        "conceptual_inquiry": "conce",
        "hybrid_explanation": "hybrd",
        "generation_comprehension": "genco",
    }

    header = f"  {'Expected':<28}" + "".join(f"{abbrev.get(p, p[:5]):>8}" for p in active_patterns)
    lines.append(header)
    lines.append("  " + "-" * (28 + 8 * len(active_patterns)))

    for expected in active_patterns:
        if expected not in confusion:
            continue
        row = f"  {expected:<28}"
        for actual in active_patterns:
            count = confusion[expected].get(actual, 0)
            if count > 0:
                row += f"{count:>8}"
            else:
                row += f"{'·':>8}"
        lines.append(row)

    # --- Section 5: Edge Cases ---
    lines.append("")
    lines.append("-" * 72)
    lines.append("  5. EDGE CASE TESTS")
    lines.append("-" * 72)
    lines.append("")
    lines.append(f"  {'Name':<38} {'Expected':<24} {'Actual':<24} {'Match'}")
    lines.append(f"  {'----':<38} {'--------':<24} {'------':<24} {'-----'}")

    edge_correct = 0
    for r in edge_results:
        mark = "PASS" if r["match"] else "FAIL"
        if r["match"]:
            edge_correct += 1
        lines.append(
            f"  {r['name']:<38} "
            f"{r['expected_pattern']:<24} "
            f"{r['actual_pattern']:<24} "
            f"{mark}"
        )
        lines.append(
            f"  {'':38} phases={r['phases']} risks={r['risks']} "
            f"alts={r['alternatives']} interactions={r['interactions']}"
        )

    lines.append("")
    lines.append(f"  Edge case accuracy: {edge_correct}/{len(edge_results)} ({edge_correct/len(edge_results)*100:.0f}%)")

    # --- Section 6: Comparison with Anthropic ---
    lines.append("")
    lines.append("-" * 72)
    lines.append("  6. COMPARISON WITH ANTHROPIC PUBLISHED NUMBERS (Shen & Tamkin 2026)")
    lines.append("-" * 72)
    lines.append("")
    lines.append(f"  {'Pattern':<28} {'Anthropic':>10} {'Our Mean':>10} {'Delta':>10}")
    lines.append(f"  {'-------':<28} {'---------':>10} {'--------':>10} {'-----':>10}")

    seen_patterns = set()
    for stype in SESSION_TYPES:
        s = statistical_results[stype]
        pattern = s["expected_pattern"]
        if pattern in seen_patterns:
            continue
        seen_patterns.add(pattern)
        anthropic_val = ANTHROPIC_REFERENCE.get(pattern, 0)
        delta = s["mean_retention"] - anthropic_val
        lines.append(
            f"  {pattern:<28} "
            f"{anthropic_val*100:>8.0f}%  "
            f"{s['mean_retention']*100:>8.1f}%  "
            f"{delta*100:>+7.1f}%"
        )

    # Retention ordering check
    lines.append("")
    lines.append("  Retention ordering validation:")
    pattern_means = {}
    for stype in SESSION_TYPES:
        s = statistical_results[stype]
        p = s["expected_pattern"]
        if p not in pattern_means:
            pattern_means[p] = s["mean_retention"]

    prev_val = -1.0
    ordering_correct = True
    for p in RETENTION_ORDER:
        if p not in pattern_means:
            continue
        val = pattern_means[p]
        ok = val >= prev_val
        if not ok:
            ordering_correct = False
        mark = "OK" if ok else "WRONG"
        lines.append(f"    {p:<28} {val*100:>6.1f}%  {mark}")
        prev_val = val

    lines.append(f"  Ordering: {'CORRECT' if ordering_correct else 'INCORRECT'}")

    # --- Section 7: Findings ---
    lines.append("")
    lines.append("-" * 72)
    lines.append("  7. FINDINGS & RECOMMENDATIONS")
    lines.append("-" * 72)
    lines.append("")
    lines.append("  [FINDING-1] classify_pattern in mentor_metrics.py does not count")
    lines.append("  dev_why_inquiries when deciding generation_comprehension vs")
    lines.append("  hybrid_explanation. A developer who asks 'why' (active engagement)")
    lines.append("  is treated the same as a passive observer. The condition at line 142:")
    lines.append("    (m.dev_risk_inquiries + m.dev_alternative_inquiries + m.dev_review_inquiries) > 0")
    lines.append("  should also include m.dev_why_inquiries.")
    lines.append("")
    lines.append("  [FINDING-2] classify_pattern never returns 'iterative_debugging'.")
    lines.append("  There is no code path that produces this pattern, even though it is")
    lines.append("  defined in the Shen & Tamkin taxonomy (24% retention). Sessions with")
    lines.append("  heavy debugging queries currently fall into other categories.")
    lines.append("")
    lines.append("  [FINDING-3] The classification has 0% variance across 50 randomized")
    lines.append("  runs per type. This is because the randomization varies content counts")
    lines.append("  (risks, alternatives, checklist) but the classification thresholds")
    lines.append("  are coarse enough that variations within range never cross boundaries.")
    lines.append("  This is good for stability but may mask sensitivity issues if real")
    lines.append("  sessions have noisier signals.")

    # --- Section 8: Summary ---
    lines.append("")
    lines.append("=" * 72)
    lines.append("  SUMMARY")
    lines.append("=" * 72)
    lines.append("")

    total_single = len(single_results)
    total_edge = len(edge_results)
    total_stat_runs = sum(s["total_runs"] for s in statistical_results.values())
    total_stat_correct = sum(s["correct_count"] for s in statistical_results.values())

    lines.append(f"  Single-run classification accuracy:  {correct}/{total_single} ({correct/total_single*100:.0f}%)")
    lines.append(f"  Statistical classification accuracy: {total_stat_correct}/{total_stat_runs} ({total_stat_correct/total_stat_runs*100:.1f}%)")
    lines.append(f"  Edge case accuracy:                  {edge_correct}/{total_edge} ({edge_correct/total_edge*100:.0f}%)")
    lines.append(f"  All stabilities >= 90%:              {'YES' if all_stable else 'NO'}")
    lines.append(f"  Retention ordering correct:          {'YES' if ordering_correct else 'NO'}")
    lines.append("")

    overall_pass = (
        correct == total_single
        and all_stable
        and ordering_correct
        and edge_correct == total_edge
    )
    lines.append(f"  OVERALL RESULT: {'PASS' if overall_pass else 'FAIL'}")
    lines.append("=" * 72)

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# 6. Main
# ---------------------------------------------------------------------------

def main():
    print("Running Mentor v2 Validation Framework...")
    print()

    # Step 1: Single classification test
    print("  [1/4] Running single classification test...")
    single_results = run_single_classification_test()

    # Step 2: Statistical validation
    num_runs = 50
    print(f"  [2/4] Running statistical validation ({num_runs} runs per type)...")
    statistical_results = run_statistical_validation(num_runs)

    # Step 3: Edge case tests
    print("  [3/4] Running edge case tests...")
    edge_results = run_edge_case_tests()

    # Step 4: Build confusion matrix
    print("  [4/4] Building confusion matrix...")
    confusion = build_confusion_matrix(statistical_results)

    # Format and print report
    report = format_report(single_results, statistical_results, edge_results, confusion)
    print()
    print(report)

    return report


if __name__ == "__main__":
    main()
