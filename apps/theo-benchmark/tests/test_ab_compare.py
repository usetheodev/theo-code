"""Phase 56 (prompt-ab-testing-plan) — paired statistical comparator tests.

6 RED tests from the plan + extras:
  - mcnemar_returns_significant_when_clear_winner
  - mcnemar_returns_nonsignificant_when_tied
  - bootstrap_ci_brackets_observed_diff
  - per_task_matrix_handles_missing_runs
  - recommendation_chosen_when_statistically_significant
  - recommendation_says_inconclusive_when_no_significance
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from runner.ab_compare import (  # noqa: E402
    bootstrap_paired_diff_ci,
    build_per_task_matrix,
    choose_recommendation,
    compute_pair_stats,
    load_records,
    main,
    mcnemar_test,
    write_per_task_matrix_csv,
)


class TestMcNemar(unittest.TestCase):
    def test_mcnemar_returns_significant_when_clear_winner(self) -> None:
        # 30 discordant pairs all in one direction → strong evidence
        result = mcnemar_test(b=20, c=2)
        self.assertLess(result["p_value"], 0.05,
                        f"expected p<0.05 for 20-vs-2 split, got {result['p_value']}")
        self.assertEqual(result["b"], 20)
        self.assertEqual(result["c"], 2)

    def test_mcnemar_returns_nonsignificant_when_tied(self) -> None:
        # Equal discordant counts → no evidence either way
        result = mcnemar_test(b=10, c=10)
        self.assertGreater(result["p_value"], 0.05)

    def test_mcnemar_returns_no_info_when_zero_discordant(self) -> None:
        # All pairs agree → test is uninformative
        result = mcnemar_test(b=0, c=0)
        self.assertEqual(result["method"], "no_discordant_pairs")
        self.assertEqual(result["p_value"], 1.0)

    def test_mcnemar_uses_exact_binomial_for_small_samples(self) -> None:
        result = mcnemar_test(b=5, c=1)
        self.assertEqual(result["method"], "exact_binomial")

    def test_mcnemar_uses_chi2_for_large_samples(self) -> None:
        result = mcnemar_test(b=15, c=15)
        self.assertEqual(result["method"], "chi2_corrected")


class TestBootstrap(unittest.TestCase):
    def test_bootstrap_ci_brackets_observed_diff(self) -> None:
        # Symmetric data centred at 1.0 — CI should bracket 1.0 with high prob
        diffs = [0.8, 0.9, 1.0, 1.1, 1.2] * 4  # n=20, mean=1.0
        ci = bootstrap_paired_diff_ci(diffs, n_boot=2000)
        self.assertLessEqual(ci["ci_low"], 1.0)
        self.assertGreaterEqual(ci["ci_high"], 1.0)
        self.assertAlmostEqual(ci["mean"], 1.0, places=2)

    def test_bootstrap_ci_handles_empty_diffs(self) -> None:
        ci = bootstrap_paired_diff_ci([])
        self.assertEqual(ci["mean"], 0.0)
        self.assertEqual(ci["n"], 0)

    def test_bootstrap_ci_is_deterministic_with_seed(self) -> None:
        # Same input + seed → same CI bounds (reproducibility)
        diffs = [0.1, -0.2, 0.3, -0.4, 0.5]
        a = bootstrap_paired_diff_ci(diffs, n_boot=1000, seed=42)
        b = bootstrap_paired_diff_ci(diffs, n_boot=1000, seed=42)
        self.assertEqual(a, b)


class TestPerTaskMatrix(unittest.TestCase):
    def test_per_task_matrix_handles_missing_runs(self) -> None:
        # variant A ran t1, t2; variant B ran only t1 — t2 has None for B
        records = {
            "A": {
                "t1": {"task_id": "t1", "passed": True},
                "t2": {"task_id": "t2", "passed": False},
            },
            "B": {
                "t1": {"task_id": "t1", "passed": False},
            },
        }
        matrix = build_per_task_matrix(["A", "B"], records)
        self.assertEqual(matrix["task_ids"], ["t1", "t2"])
        self.assertIsNone(matrix["matrix"]["t2"]["B"])
        self.assertEqual(matrix["matrix"]["t1"]["A"]["passed"], True)

    def test_csv_writer_emits_header_and_rows(self) -> None:
        records = {
            "A": {"t1": {"task_id": "t1", "passed": True}},
            "B": {"t1": {"task_id": "t1", "passed": False}},
        }
        matrix = build_per_task_matrix(["A", "B"], records)
        with tempfile.NamedTemporaryFile(suffix=".csv", mode="w", delete=False) as f:
            path = Path(f.name)
        write_per_task_matrix_csv(path, ["A", "B"], matrix)
        rows = path.read_text().splitlines()
        self.assertEqual(rows[0], "task_id,A,B")
        self.assertEqual(rows[1], "t1,PASS,FAIL")


class TestRecommendation(unittest.TestCase):
    def test_recommendation_chosen_when_statistically_significant(self) -> None:
        # One pair has clearly significant winner
        pair_stats = [{
            "variant_a": "sota", "variant_b": "sota-lean",
            "a_pass": 18, "b_pass": 4,
            "mcnemar": {"p_value": 0.001, "b": 16, "c": 2},
        }]
        rec = choose_recommendation(pair_stats)
        self.assertIn("sota", rec)
        self.assertIn("Adopt", rec)

    def test_recommendation_says_inconclusive_when_no_significance(self) -> None:
        # All p-values > 0.05
        pair_stats = [{
            "variant_a": "sota", "variant_b": "sota-lean",
            "a_pass": 10, "b_pass": 10,
            "mcnemar": {"p_value": 0.5, "b": 5, "c": 5},
        }]
        rec = choose_recommendation(pair_stats)
        self.assertIn("Inconclusive", rec)


class TestComputePairStats(unittest.TestCase):
    def test_pair_stats_includes_pass_rates_and_mcnemar(self) -> None:
        records = {
            "A": {
                "t1": {"task_id": "t1", "passed": True, "cost_usd": 0.10, "iterations": 5},
                "t2": {"task_id": "t2", "passed": True, "cost_usd": 0.20, "iterations": 8},
                "t3": {"task_id": "t3", "passed": False, "cost_usd": 0.05, "iterations": 3},
            },
            "B": {
                "t1": {"task_id": "t1", "passed": False, "cost_usd": 0.30, "iterations": 12},
                "t2": {"task_id": "t2", "passed": False, "cost_usd": 0.25, "iterations": 10},
                "t3": {"task_id": "t3", "passed": False, "cost_usd": 0.15, "iterations": 7},
            },
        }
        stats = compute_pair_stats("A", "B", records)
        self.assertEqual(stats["n_paired"], 3)
        self.assertEqual(stats["a_pass"], 2)
        self.assertEqual(stats["b_pass"], 0)
        # A pass on t1 + t2 where B fails → b_count = 2
        self.assertEqual(stats["mcnemar"]["b"], 2)
        self.assertEqual(stats["mcnemar"]["c"], 0)


class TestMainIntegration(unittest.TestCase):
    def _build_ab_dir(self, td: Path) -> Path:
        ab = td / "ab"
        ab.mkdir()
        (ab / "manifest.json").write_text(json.dumps({
            "schema": "ab.manifest.v1",
            "variants": ["sota", "sota-lean"],
            "task_ids": ["t1", "t2", "t3"],
            "n_tasks": 3,
            "theo_sha": "abc1234",
            "model": "gpt-5.4",
            "dataset": "terminal-bench-core==0.1.1",
            "started_at": "2026-04-24T12:00:00+00:00",
        }))
        for v, results in [
            ("sota", [(t, True) for t in ("t1", "t2", "t3")]),
            ("sota-lean", [("t1", False), ("t2", False), ("t3", True)]),
        ]:
            ana = ab / v / "analyzed"
            ana.mkdir(parents=True)
            for tid, passed in results:
                (ana / f"{tid}.json").write_text(json.dumps({
                    "task_id": tid, "passed": passed,
                    "cost_usd": 0.10, "iterations": 5,
                    "duration_ms_wall": 1000,
                }))
        return ab

    def test_main_writes_comparison_md_and_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            ab = self._build_ab_dir(Path(td))
            rc = main(["--ab-dir", str(ab)])
            self.assertEqual(rc, 0)
            self.assertTrue((ab / "comparison.md").exists())
            self.assertTrue((ab / "per_task_matrix.csv").exists())
            self.assertTrue((ab / "mcnemar_results.json").exists())
            md = (ab / "comparison.md").read_text()
            self.assertIn("# Prompt A/B Comparison", md)
            self.assertIn("McNemar", md)


if __name__ == "__main__":
    unittest.main(verbosity=2)
