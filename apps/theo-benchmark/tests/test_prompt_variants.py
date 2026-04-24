"""Phase 53 (prompt-ab-testing-plan) — lock tests for the 3 prompt variants.

Each variant must:
- Exist as a markdown file in apps/theo-benchmark/prompts/
- Carry the SOTA persistence/verification doctrine (no regression to legacy)
- Respect the documented token budget (sota-lean is the only one trimmed)

Token estimate uses chars/4 (industry standard heuristic). Tests are stdlib
unittest only — no third-party deps.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROMPTS = ROOT / "prompts"


def _est_tokens(text: str) -> int:
    return len(text) // 4


class TestSotaVariant(unittest.TestCase):
    def setUp(self) -> None:
        self.path = PROMPTS / "sota.md"

    def test_sota_md_exists(self) -> None:
        self.assertTrue(self.path.is_file(), f"missing variant: {self.path}")

    def test_sota_md_has_persistence_doctrine(self) -> None:
        contents = self.path.read_text()
        # Persistence doctrine — verify-by-execute is the headline rule
        self.assertIn("VERIFY by EXECUTING", contents)
        self.assertIn("Persist until", contents)

    def test_sota_md_includes_benchmark_addendum(self) -> None:
        contents = self.path.read_text()
        # Bench mode block should be baked in (sota = SOTA + addendum)
        self.assertIn("Benchmark evaluation context", contents)
        self.assertIn("Self-verification before `done`", contents)

    def test_sota_md_under_3500_tokens(self) -> None:
        contents = self.path.read_text()
        est = _est_tokens(contents)
        self.assertLessEqual(
            est, 3500, f"sota.md ~{est} tokens — exceeds 3500 budget"
        )


class TestSotaNoBenchVariant(unittest.TestCase):
    def setUp(self) -> None:
        self.path = PROMPTS / "sota-no-bench.md"

    def test_sota_no_bench_md_exists(self) -> None:
        self.assertTrue(self.path.is_file(), f"missing variant: {self.path}")

    def test_sota_no_bench_md_lacks_benchmark_addendum(self) -> None:
        contents = self.path.read_text()
        # The whole point of this variant: NO bench addendum
        self.assertNotIn("Benchmark evaluation context", contents)
        self.assertNotIn("Self-verification before `done`", contents)

    def test_sota_no_bench_md_keeps_persistence_doctrine(self) -> None:
        contents = self.path.read_text()
        self.assertIn("VERIFY by EXECUTING", contents)


class TestSotaLeanVariant(unittest.TestCase):
    def setUp(self) -> None:
        self.path = PROMPTS / "sota-lean.md"

    def test_sota_lean_md_exists(self) -> None:
        self.assertTrue(self.path.is_file(), f"missing variant: {self.path}")

    def test_sota_lean_md_under_1700_tokens(self) -> None:
        # Plan target: ~1500. Hard ceiling 1700 to fail loud on drift.
        contents = self.path.read_text()
        est = _est_tokens(contents)
        self.assertLessEqual(
            est, 1700, f"sota-lean.md ~{est} tokens — exceeds 1700 budget"
        )

    def test_sota_lean_md_keeps_persistence_doctrine(self) -> None:
        # The whole experiment: shorter prompt, same doctrine.
        # If lean drops persistence, we're not testing prompt-length-only.
        contents = self.path.read_text()
        self.assertIn("VERIFY", contents)
        self.assertIn("Persist", contents)

    def test_sota_lean_md_keeps_git_safety_absolutes(self) -> None:
        # Git safety is a non-negotiable doctrine — must survive trim
        contents = self.path.read_text()
        self.assertIn("git reset --hard", contents)


class TestVariantsAreDistinct(unittest.TestCase):
    def test_all_three_variants_have_different_content(self) -> None:
        sota = (PROMPTS / "sota.md").read_text()
        lean = (PROMPTS / "sota-lean.md").read_text()
        nob = (PROMPTS / "sota-no-bench.md").read_text()
        self.assertNotEqual(sota, lean, "sota and sota-lean must differ")
        self.assertNotEqual(sota, nob, "sota and sota-no-bench must differ")
        self.assertNotEqual(lean, nob, "sota-lean and sota-no-bench must differ")


if __name__ == "__main__":
    unittest.main()
