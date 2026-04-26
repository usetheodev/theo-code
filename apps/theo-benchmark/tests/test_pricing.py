"""Tests for pricing.compute_cost (Phase 46) — stdlib unittest, no deps."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from pricing import compute_cost, load_table  # noqa: E402


class TestPricing(unittest.TestCase):
    def test_load_table_returns_dict_with_known_models(self) -> None:
        t = load_table()
        self.assertIn("gpt-5.4", t)
        self.assertIn("claude-opus-4-7", t)
        self.assertIn("__fallback__", t)

    def test_compute_cost_for_known_model_gpt_5_4(self) -> None:
        # 1M input + 1M output at $5/$15 = $20
        cost = compute_cost(1_000_000, 1_000_000, "gpt-5.4")
        self.assertAlmostEqual(cost, 20.0, places=4)

    def test_compute_cost_scales_linearly_with_tokens(self) -> None:
        cost_1k = compute_cost(1_000, 1_000, "gpt-5.4")
        cost_2k = compute_cost(2_000, 2_000, "gpt-5.4")
        self.assertAlmostEqual(cost_2k, cost_1k * 2, places=6)

    def test_compute_cost_handles_zero_tokens(self) -> None:
        self.assertEqual(compute_cost(0, 0, "gpt-5.4"), 0.0)

    def test_compute_cost_input_only(self) -> None:
        # 1M input tokens at $5 = $5
        self.assertAlmostEqual(compute_cost(1_000_000, 0, "gpt-5.4"), 5.0, places=4)

    def test_compute_cost_output_only(self) -> None:
        # 1M output tokens at $15 = $15
        self.assertAlmostEqual(compute_cost(0, 1_000_000, "gpt-5.4"), 15.0, places=4)

    def test_compute_cost_returns_zero_for_unknown_model(self) -> None:
        self.assertEqual(
            compute_cost(1_000_000, 1_000_000, "model-that-does-not-exist"), 0.0
        )

    def test_compute_cost_rejects_negative_tokens(self) -> None:
        with self.assertRaises(ValueError):
            compute_cost(-1, 0, "gpt-5.4")
        with self.assertRaises(ValueError):
            compute_cost(0, -1, "gpt-5.4")

    def test_compute_cost_supports_anthropic_models(self) -> None:
        # Opus 4.7: $15 input + $75 output per 1M
        self.assertAlmostEqual(
            compute_cost(1_000_000, 1_000_000, "claude-opus-4-7"), 90.0, places=4
        )

    def test_compute_cost_supports_explicit_table_arg(self) -> None:
        custom = {"x-model": {"input_per_mtok": 100.0, "output_per_mtok": 100.0}}
        self.assertAlmostEqual(
            compute_cost(1_000_000, 1_000_000, "x-model", table=custom), 200.0, places=4
        )

    def test_compute_cost_realistic_swe_bench_run(self) -> None:
        # Typical SWE-bench Lite task: ~30K input, ~3K output via gpt-5.4
        cost = compute_cost(30_000, 3_000, "gpt-5.4")
        expected = (30_000 / 1_000_000) * 5.00 + (3_000 / 1_000_000) * 15.00
        self.assertAlmostEqual(cost, expected, places=6)
        # Cost must be bounded: < $0.50 for one task at this size
        self.assertLess(cost, 0.50)


if __name__ == "__main__":
    unittest.main(verbosity=2)
