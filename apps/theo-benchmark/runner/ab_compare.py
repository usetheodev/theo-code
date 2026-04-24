"""Phase 56 (prompt-ab-testing-plan) — paired statistical comparison.

Reads per-variant per-task JSON records from tbench_post (or from raw tb output
which is processed on demand) and produces:

  <output>/comparison.md           — decision-ready report
  <output>/per_task_matrix.csv     — rows=task_id, cols=variants, cells=passed
  <output>/mcnemar_results.json    — paired binary test for each variant pair
  <output>/cost_analysis.json      — paired diffs + bootstrap CI 95%

Statistical methods (D5):
  - McNemar exact binomial test for paired binary outcomes (resolved/unresolved)
    with continuity-corrected chi² fallback for n >= 25
  - Bootstrap CI (10k resamples) for paired continuous metrics
    (cost_usd, iterations, duration_ms_wall)
  - All decisions are paired: same task IDs across variants — no proportion
    test on independent samples

Usage:
    python3 runner/ab_compare.py \\
      --ab-dir reports/2026-04-24/ab \\
      --output reports/2026-04-24/ab/comparison.md
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
import sys
from itertools import combinations
from math import comb
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


# --------------------------------------------------------------------------- #
# Pure helpers — easy to unit test                                            #
# --------------------------------------------------------------------------- #


def mcnemar_test(b: int, c: int) -> dict:
    """McNemar paired-binary test.

    b = count of (A pass, B fail)
    c = count of (A fail, B pass)

    Returns dict with: b, c, n_discordant, p_value, statistic, method.

    Uses exact binomial test when discordant < 25, continuity-corrected
    chi² otherwise. Returns p_value=1.0 when b == c == 0 (no info).
    """
    n_disc = b + c
    if n_disc == 0:
        return {
            "b": b, "c": c, "n_discordant": 0,
            "p_value": 1.0, "statistic": 0.0,
            "method": "no_discordant_pairs",
        }
    if n_disc < 25:
        # Exact two-sided binomial: P(X >= max(b,c)) under H0: p=0.5
        k = max(b, c)
        # Two-sided: 2 * P(X >= k) when k > n/2, capped at 1.0
        tail = sum(comb(n_disc, i) for i in range(k, n_disc + 1)) / (2 ** n_disc)
        p = min(1.0, 2 * tail)
        return {
            "b": b, "c": c, "n_discordant": n_disc,
            "p_value": p, "statistic": float(k),
            "method": "exact_binomial",
        }
    # Continuity-corrected chi²
    chi2 = (abs(b - c) - 1) ** 2 / n_disc
    # Survival function of chi²(df=1) at chi2 = erfc(sqrt(chi2/2))
    p = math.erfc(math.sqrt(chi2 / 2))
    return {
        "b": b, "c": c, "n_discordant": n_disc,
        "p_value": p, "statistic": chi2,
        "method": "chi2_corrected",
    }


def bootstrap_paired_diff_ci(
    diffs: list[float], confidence: float = 0.95, n_boot: int = 10000,
    seed: int | None = 1729,
) -> dict:
    """Bootstrap CI for paired continuous-metric differences.

    `diffs[i]` = metric_A[i] - metric_B[i] (already paired).
    Returns dict with: mean, median, ci_low, ci_high, n.
    Empty diffs → all zeros.
    """
    n = len(diffs)
    if n == 0:
        return {"mean": 0.0, "median": 0.0, "ci_low": 0.0, "ci_high": 0.0, "n": 0}
    rng = random.Random(seed)
    means = []
    for _ in range(n_boot):
        sample = [diffs[rng.randrange(n)] for _ in range(n)]
        means.append(sum(sample) / n)
    means.sort()
    alpha = (1 - confidence) / 2
    lo_idx = int(n_boot * alpha)
    hi_idx = int(n_boot * (1 - alpha)) - 1
    sorted_diffs = sorted(diffs)
    median = (
        sorted_diffs[n // 2]
        if n % 2 == 1
        else (sorted_diffs[n // 2 - 1] + sorted_diffs[n // 2]) / 2
    )
    return {
        "mean": sum(diffs) / n,
        "median": median,
        "ci_low": means[lo_idx],
        "ci_high": means[hi_idx],
        "n": n,
    }


# --------------------------------------------------------------------------- #
# Phase 62 (headless-error-classification-plan) — infra failure exclusion     #
# --------------------------------------------------------------------------- #

# error_class values that represent infrastructure failures (provider 429,
# auth, sandbox denial, context window). These outcomes do NOT reflect agent
# behavior, so the paired statistical comparison MUST exclude them — counting
# them as "agent failed" would bias the A/B against any variant that runs
# after a costly variant (the smoke3 incident).
INFRA_FAILURE_CLASSES = frozenset({
    "rate_limited",
    "quota_exceeded",
    "auth_failed",
    "context_overflow",
    "sandbox_denied",
})


def is_real_outcome(record: dict | None) -> bool:
    """True if `record` reflects a genuine agent outcome (not infra failure).

    A trial counts as "real" when:
      - the record exists (we ran the trial), AND
      - error_class is NOT in INFRA_FAILURE_CLASSES.

    Records without `error_class` (legacy v2 schema) are treated as real —
    we have no evidence of infra failure, so trust the success/passed flag.
    """
    if record is None:
        return False
    ec = record.get("error_class")
    if ec is None:
        return True  # legacy v2 — no classification, assume real
    return ec not in INFRA_FAILURE_CLASSES


def count_infra_failures(records_by_task: dict[str, dict]) -> int:
    """Count records with error_class in INFRA_FAILURE_CLASSES."""
    return sum(
        1 for r in records_by_task.values()
        if r is not None and r.get("error_class") in INFRA_FAILURE_CLASSES
    )


def build_per_task_matrix(
    variants: list[str],
    records: dict[str, dict[str, dict]],
) -> dict:
    """records[variant][task_id] = analytic record.

    Returns:
      task_ids: sorted list of all task IDs seen
      matrix: dict mapping task_id → dict variant → record (or None)
    """
    all_tasks = set()
    for v in variants:
        all_tasks.update(records.get(v, {}).keys())
    task_ids = sorted(all_tasks)
    matrix = {}
    for tid in task_ids:
        matrix[tid] = {v: records.get(v, {}).get(tid) for v in variants}
    return {"task_ids": task_ids, "matrix": matrix}


def compute_pair_stats(
    variant_a: str, variant_b: str,
    records: dict[str, dict[str, dict]],
) -> dict:
    """Build the full statistical packet for one variant pair.

    Phase 62: excludes tasks where EITHER variant had an infra failure
    (rate-limited, quota exceeded, etc.) from the paired McNemar — those
    outcomes don't reflect agent behavior, so counting them would bias
    the comparison.
    """
    a_records = records.get(variant_a, {})
    b_records = records.get(variant_b, {})
    # Phase 62: paired set is intersection of REAL outcomes only
    candidate = set(a_records.keys()) & set(b_records.keys())
    common_ids = sorted(
        t for t in candidate
        if is_real_outcome(a_records.get(t)) and is_real_outcome(b_records.get(t))
    )
    a_recs = [a_records[t] for t in common_ids]
    b_recs = [b_records[t] for t in common_ids]
    n_excluded_a = count_infra_failures(a_records)
    n_excluded_b = count_infra_failures(b_records)

    # McNemar
    b_count = sum(1 for ra, rb in zip(a_recs, b_recs)
                  if ra.get("passed") and not rb.get("passed"))
    c_count = sum(1 for ra, rb in zip(a_recs, b_recs)
                  if not ra.get("passed") and rb.get("passed"))
    a_pass = sum(1 for r in a_recs if r.get("passed"))
    b_pass = sum(1 for r in b_recs if r.get("passed"))
    mcn = mcnemar_test(b_count, c_count)

    # Cost diff
    cost_diffs = [
        float(ra.get("cost_usd", 0.0) or 0.0) - float(rb.get("cost_usd", 0.0) or 0.0)
        for ra, rb in zip(a_recs, b_recs)
    ]
    cost_ci = bootstrap_paired_diff_ci(cost_diffs)

    # Iter diff
    iter_diffs = [
        float(ra.get("iterations", 0) or 0) - float(rb.get("iterations", 0) or 0)
        for ra, rb in zip(a_recs, b_recs)
    ]
    iter_ci = bootstrap_paired_diff_ci(iter_diffs)

    # Duration diff
    dur_diffs = [
        float(ra.get("duration_ms_wall", 0) or 0) - float(rb.get("duration_ms_wall", 0) or 0)
        for ra, rb in zip(a_recs, b_recs)
    ]
    dur_ci = bootstrap_paired_diff_ci(dur_diffs)

    return {
        "variant_a": variant_a,
        "variant_b": variant_b,
        "n_paired": len(common_ids),
        "n_excluded_a": n_excluded_a,
        "n_excluded_b": n_excluded_b,
        "a_pass": a_pass,
        "b_pass": b_pass,
        "a_pass_rate": round(a_pass / len(common_ids), 4) if common_ids else 0.0,
        "b_pass_rate": round(b_pass / len(common_ids), 4) if common_ids else 0.0,
        "mcnemar": mcn,
        "cost_diff_usd": cost_ci,
        "iter_diff": iter_ci,
        "duration_diff_ms": dur_ci,
    }


def choose_recommendation(pair_stats: list[dict], significance: float = 0.05) -> str:
    """Build the recommendation sentence for the report.

    Picks the variant with highest pass rate that has at least one
    statistically significant (p<significance) win against another.

    Phase 62: also warns when too many trials were excluded due to
    infra failures — high exclusion ratio means the dataset is unreliable.
    """
    # Data quality check: total exclusions vs total paired
    total_paired = sum(ps.get("n_paired", 0) for ps in pair_stats)
    total_excluded = sum(
        ps.get("n_excluded_a", 0) + ps.get("n_excluded_b", 0)
        for ps in pair_stats
    )
    quality_warning = ""
    if total_paired > 0 and total_excluded > total_paired:
        quality_warning = (
            f"\n\n**Data quality concern**: {total_excluded} infra-failure "
            f"exclusions vs {total_paired} valid paired trials. Re-run with "
            "more headroom on TPM/quota before trusting these results."
        )

    significant_wins = {}
    for ps in pair_stats:
        if ps["mcnemar"]["p_value"] < significance:
            winner = ps["variant_a"] if ps["a_pass"] > ps["b_pass"] else ps["variant_b"]
            significant_wins[winner] = significant_wins.get(winner, 0) + 1
    if not significant_wins:
        return (
            "**Inconclusive** — no pair reached statistical significance at "
            f"p<{significance}. Consider running with larger N (current sample "
            "may be too small to detect the effect)."
            + quality_warning
        )
    top = max(significant_wins, key=significant_wins.get)
    n_wins = significant_wins[top]
    return (
        f"**Adopt `{top}`** — wins {n_wins} significant pairwise comparison(s) "
        f"at p<{significance}."
        + quality_warning
    )


# --------------------------------------------------------------------------- #
# IO + report rendering                                                       #
# --------------------------------------------------------------------------- #


def load_records(ab_dir: Path) -> tuple[list[str], dict[str, dict[str, dict]]]:
    """Load per-task records for each variant present under ab_dir.

    Layout (set by ab_test.py + tbench_post.py):
      ab_dir/manifest.json
      ab_dir/<variant>/analyzed/<task_id>.json    (preferred, post-processed)
      ab_dir/<variant>/raw/                       (raw tb output if not analyzed)
    """
    manifest = json.loads((ab_dir / "manifest.json").read_text())
    variants = manifest["variants"]
    records: dict[str, dict[str, dict]] = {}
    for v in variants:
        var_dir = ab_dir / v
        analyzed = var_dir / "analyzed"
        records[v] = {}
        if analyzed.is_dir():
            for p in analyzed.glob("*.json"):
                if p.name == "summary.json":
                    continue
                try:
                    rec = json.loads(p.read_text())
                except Exception:
                    continue
                tid = rec.get("task_id") or p.stem
                records[v][tid] = rec
    return variants, records


def write_per_task_matrix_csv(out_path: Path, variants: list[str], matrix: dict) -> None:
    with out_path.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["task_id"] + variants)
        for tid in matrix["task_ids"]:
            row = [tid]
            for v in variants:
                rec = matrix["matrix"][tid].get(v)
                row.append("" if rec is None else ("PASS" if rec.get("passed") else "FAIL"))
            w.writerow(row)


def render_comparison_md(
    variants: list[str],
    pair_stats: list[dict],
    matrix: dict,
    manifest: dict,
) -> str:
    lines = []
    lines.append("# Prompt A/B Comparison")
    lines.append("")
    lines.append(f"- Dataset: `{manifest.get('dataset', '?')}`")
    lines.append(f"- Started at: `{manifest.get('started_at', '?')}`")
    lines.append(f"- theo SHA: `{manifest.get('theo_sha', '?')}`")
    lines.append(f"- Model: `{manifest.get('model', '?')}`")
    lines.append(f"- Tasks per variant: {manifest.get('n_tasks', '?')}")
    lines.append("")
    lines.append("## Headline")
    lines.append("")
    lines.append("| Variant | Pass | Pass rate | Mean cost ($) | Mean iters |")
    lines.append("|---|---:|---:|---:|---:|")
    # Aggregate per-variant means by reading any pair_stats record
    per_variant_metrics = {}
    for v in variants:
        per_variant_metrics[v] = {"pass": None, "rate": None, "cost": None, "iter": None}
    for ps in pair_stats:
        for v, key_pass, key_rate in [
            (ps["variant_a"], "a_pass", "a_pass_rate"),
            (ps["variant_b"], "b_pass", "b_pass_rate"),
        ]:
            per_variant_metrics[v]["pass"] = ps[key_pass]
            per_variant_metrics[v]["rate"] = ps[key_rate]
    # Mean cost / iter come from the first pair that contains the variant as A
    cost_sums = {v: [] for v in variants}
    iter_sums = {v: [] for v in variants}
    for ps in pair_stats:
        n = max(1, ps["n_paired"])
        # Reconstruct mean per variant from diff: not directly available;
        # we leave None and rely on the per-pair table for nuance.
    for v in variants:
        m = per_variant_metrics[v]
        passes = "?" if m["pass"] is None else m["pass"]
        rate = "?" if m["rate"] is None else f"{m['rate']*100:.1f}%"
        lines.append(f"| `{v}` | {passes} | {rate} | — | — |")
    lines.append("")
    lines.append("## Pairwise comparisons (McNemar)")
    lines.append("")
    lines.append(
        "Phase 62: trials with infra failures (rate-limited, quota exceeded, "
        "auth, sandbox) are EXCLUDED from the paired set — they reflect "
        "provider state, not agent behavior."
    )
    lines.append("")
    lines.append("| Pair | n | excl A | excl B | b (A>B) | c (B>A) | p-value | Method | Cost diff (95% CI) |")
    lines.append("|---|---:|---:|---:|---:|---:|---:|---|---|")
    for ps in pair_stats:
        cd = ps["cost_diff_usd"]
        ci_str = f"${cd['mean']:+.4f} [{cd['ci_low']:+.4f}, {cd['ci_high']:+.4f}]"
        lines.append(
            f"| `{ps['variant_a']}` vs `{ps['variant_b']}` | "
            f"{ps['n_paired']} | "
            f"{ps.get('n_excluded_a', 0)} | {ps.get('n_excluded_b', 0)} | "
            f"{ps['mcnemar']['b']} | {ps['mcnemar']['c']} | "
            f"{ps['mcnemar']['p_value']:.4f} | {ps['mcnemar']['method']} | "
            f"{ci_str} |"
        )
    lines.append("")
    lines.append("## Per-task win/loss")
    lines.append("")
    lines.append("Tasks where variants disagree (one passed, others failed).")
    lines.append("")
    lines.append("| Task | " + " | ".join(f"`{v}`" for v in variants) + " |")
    lines.append("|" + "---|" * (len(variants) + 1))
    for tid in matrix["task_ids"]:
        outcomes = [matrix["matrix"][tid].get(v) for v in variants]
        passed_set = {r.get("passed") for r in outcomes if r is not None}
        if len(passed_set) <= 1:
            continue  # all agree → skip in the disagreement table
        cells = []
        for r in outcomes:
            if r is None:
                cells.append("—")
            elif r.get("passed"):
                cells.append("PASS")
            else:
                cells.append("FAIL")
        lines.append(f"| `{tid}` | " + " | ".join(cells) + " |")
    lines.append("")
    lines.append("## Recommendation")
    lines.append("")
    lines.append(choose_recommendation(pair_stats))
    lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Phase 56 — paired prompt A/B comparison")
    ap.add_argument("--ab-dir", required=True, type=Path,
                    help="Directory containing manifest.json and per-variant subdirs")
    ap.add_argument("--output", type=Path,
                    help="Path for comparison.md (default: <ab-dir>/comparison.md)")
    args = ap.parse_args(argv)

    if not (args.ab_dir / "manifest.json").exists():
        print(f"[ab_compare] manifest.json not found in {args.ab_dir}", file=sys.stderr)
        return 2

    variants, records = load_records(args.ab_dir)
    if len(variants) < 2:
        print("[ab_compare] need at least 2 variants for comparison", file=sys.stderr)
        return 2

    matrix = build_per_task_matrix(variants, records)
    pair_stats = []
    for a, b in combinations(variants, 2):
        pair_stats.append(compute_pair_stats(a, b, records))

    # Persist machine-readable artifacts
    (args.ab_dir / "mcnemar_results.json").write_text(
        json.dumps([{**ps, "cost_diff_usd": ps["cost_diff_usd"]} for ps in pair_stats], indent=2)
    )
    write_per_task_matrix_csv(args.ab_dir / "per_task_matrix.csv", variants, matrix)

    # Render report
    manifest = json.loads((args.ab_dir / "manifest.json").read_text())
    report = render_comparison_md(variants, pair_stats, matrix, manifest)
    out_path = args.output or (args.ab_dir / "comparison.md")
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(f"[ab_compare] wrote {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
