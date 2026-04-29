"""Tests for the metrics collector."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from e2e.metrics_collector import (
    aggregate_by_category,
    aggregate_by_suite,
    consolidate,
    failure_taxonomy,
    to_markdown,
)


def _make_result(task_id="t1", category="smoke-read", suite="smoke", passed=True, cost=0.01, duration=5000):
    return {
        "schema_version": "theo.benchmark-run.v1",
        "run_id": "fake-uuid",
        "model_id": "gpt-4o",
        "timestamp": "2026-04-29T00:00:00Z",
        "theo_sha": "abc1234",
        "task_id": task_id,
        "task_category": category,
        "benchmark_suite": suite,
        "pass": passed,
        "duration_ms": duration,
        "tokens": {"input": 100, "output": 50, "total": 150},
        "cost_usd": cost,
    }


class TestAggregation:
    def test_by_category_groups_correctly(self):
        results = [
            _make_result(category="smoke-read", passed=True),
            _make_result(category="smoke-read", passed=False),
            _make_result(category="smoke-fix-bug", passed=True),
        ]
        agg = aggregate_by_category(results)
        assert "smoke-read" in agg
        assert agg["smoke-read"]["total"] == 2
        assert agg["smoke-read"]["passed"] == 1
        assert agg["smoke-read"]["pass_rate"] == 0.5

    def test_by_suite_groups_correctly(self):
        results = [
            _make_result(suite="smoke", passed=True),
            _make_result(suite="e2e-probe", passed=True),
            _make_result(suite="e2e-probe", passed=False),
        ]
        agg = aggregate_by_suite(results)
        assert agg["smoke"]["pass_rate"] == 1.0
        assert agg["e2e-probe"]["pass_rate"] == 0.5

    def test_empty_results(self):
        agg = aggregate_by_category([])
        assert agg == {}


class TestConsolidate:
    def test_totals_correct(self):
        results = [
            _make_result(passed=True, cost=0.01),
            _make_result(passed=False, cost=0.02),
            _make_result(passed=True, cost=0.03),
        ]
        report = consolidate(results)
        assert report["totals"]["runs"] == 3
        assert report["totals"]["passed"] == 2
        assert report["totals"]["pass_rate"] == round(2 / 3, 4)
        assert report["totals"]["total_cost_usd"] == 0.06

    def test_skipped_excluded_from_pass_rate(self):
        results = [
            _make_result(passed=True),
            {**_make_result(passed=False), "metadata": {"skipped": True}},
        ]
        report = consolidate(results)
        assert report["totals"]["skipped"] == 1
        assert report["totals"]["executed"] == 1
        assert report["totals"]["pass_rate"] == 1.0


class TestFailureTaxonomy:
    def test_counts_error_types(self):
        results = [
            {**_make_result(passed=False), "error": {"type": "timeout"}},
            {**_make_result(passed=False), "error": {"type": "timeout"}},
            {**_make_result(passed=False), "error": {"type": "llm"}},
            _make_result(passed=True),
        ]
        tax = failure_taxonomy(results)
        assert tax["timeout"] == 2
        assert tax["llm"] == 1
        assert "unknown" not in tax  # passed results excluded


class TestMarkdown:
    def test_renders_without_error(self):
        results = [_make_result(), _make_result(passed=False)]
        report = consolidate(results)
        md = to_markdown(report)
        assert "# Consolidated Benchmark Report" in md
        assert "Pass Rate" in md
