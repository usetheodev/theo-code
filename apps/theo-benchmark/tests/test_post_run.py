"""Tests for analysis.post_run + analysis.aggregate (Phase 47).

Stdlib unittest only. Uses captured fixtures (no live OTLP collector).
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from analysis.post_run import (  # noqa: E402
    _percentile, analyze_run, index_spans_by_run, span_duration_ms,
)
from analysis.aggregate import (  # noqa: E402
    benchmark_summary, collect_records, write_markdown,
)


def _ns(ms: int) -> int:
    return ms * 1_000_000


def _span(name: str, run_id: str, start_ms: int, end_ms: int,
          extra_attrs: dict | None = None) -> dict:
    """Build an OTLP/JSON span envelope as the collector file exporter
    writes them (one envelope per resourceSpans group)."""
    attrs = [{"key": "theo.run_id", "value": {"stringValue": run_id}}]
    for k, v in (extra_attrs or {}).items():
        attrs.append({"key": k, "value": {"stringValue": str(v)}})
    return {
        "resourceSpans": [{
            "scopeSpans": [{
                "spans": [{
                    "name": name,
                    "startTimeUnixNano": str(_ns(start_ms)),
                    "endTimeUnixNano": str(_ns(end_ms)),
                    "attributes": attrs,
                }]
            }]
        }]
    }


def _summary(**kw) -> dict:
    base = {
        "schema": "theo.headless.v2",
        "run_id": "run-x",
        "model": "gpt-5.4",
        "provider": "openai",
        "success": True,
        "iterations": 4,
        "tokens": {"input": 5000, "output": 800, "total": 5800},
        "tools": {"total": 6, "success": 6},
        "llm": {"calls": 4, "retries": 0},
        "duration_ms": 12345,
    }
    base.update(kw)
    return base


class TestPercentile(unittest.TestCase):
    def test_empty_list_returns_zero(self) -> None:
        self.assertEqual(_percentile([], 50), 0.0)

    def test_singleton_returns_value(self) -> None:
        self.assertEqual(_percentile([42.0], 50), 42.0)
        self.assertEqual(_percentile([42.0], 95), 42.0)

    def test_p50_of_three_returns_median(self) -> None:
        self.assertEqual(_percentile([1.0, 2.0, 3.0], 50), 2.0)

    def test_p95_of_100_returns_top(self) -> None:
        vs = [float(i) for i in range(100)]
        # p95 of 0..99 = ~94.05 via linear interp
        self.assertAlmostEqual(_percentile(vs, 95), 94.05, places=2)


class TestSpanDuration(unittest.TestCase):
    def test_duration_in_ms(self) -> None:
        sp = {"startTimeUnixNano": str(_ns(100)), "endTimeUnixNano": str(_ns(250))}
        self.assertEqual(span_duration_ms(sp), 150.0)

    def test_zero_when_missing(self) -> None:
        self.assertEqual(span_duration_ms({}), 0.0)


class TestIndexSpansByRun(unittest.TestCase):
    def test_groups_spans_by_run_id(self) -> None:
        spans = [
            _span("agent.run", "run-A", 100, 200),
            _span("tool.call", "run-A", 110, 130),
            _span("agent.run", "run-B", 100, 200),
        ]
        idx = index_spans_by_run(spans)
        self.assertIn("run-A", idx)
        self.assertIn("run-B", idx)
        self.assertEqual(len(idx["run-A"]), 2)
        self.assertEqual(len(idx["run-B"]), 1)


class TestAnalyzeRun(unittest.TestCase):
    def test_extracts_token_and_cost_from_summary(self) -> None:
        rec = analyze_run(trajectory=[], spans_for_run=[], headless_summary=_summary())
        # 5000 input * $5/1M + 800 output * $15/1M = $0.025 + $0.012 = $0.037
        self.assertAlmostEqual(rec["cost_usd"], 0.037, places=4)
        self.assertEqual(rec["tokens"]["total"], 5800)
        self.assertEqual(rec["iterations"], 4)
        self.assertEqual(rec["model"], "gpt-5.4")

    def test_computes_first_action_latency_from_spans(self) -> None:
        spans = [
            _span("agent.run", "run-x", start_ms=100, end_ms=500),
            _span("tool.call", "run-x", start_ms=120, end_ms=140),
        ]
        idx = index_spans_by_run(spans)
        rec = analyze_run([], idx["run-x"], _summary())
        self.assertEqual(rec["first_action_latency_ms"], 20.0)

    def test_computes_p50_p95_tool_dispatch(self) -> None:
        # 5 tool spans, durations 10, 20, 30, 40, 50 ms
        spans: list[dict] = []
        for i, dur in enumerate([10, 20, 30, 40, 50]):
            start = 1000 + i * 100
            spans.append(_span("tool.call", "run-x", start, start + dur))
        idx = index_spans_by_run(spans)
        rec = analyze_run([], idx["run-x"], _summary())
        self.assertEqual(rec["p50_tool_dispatch_ms"], 30.0)
        # p95 of [10,20,30,40,50] = ~48.0
        self.assertAlmostEqual(rec["p95_tool_dispatch_ms"], 48.0, places=1)

    def test_includes_failure_modes_from_trajectory_summary(self) -> None:
        traj = [
            {"kind": "summary", "payload": {
                "failure_modes": {
                    "premature_termination": True,
                    "weak_verification": False,
                    "task_derailment": True,
                },
            }},
        ]
        rec = analyze_run(traj, [], _summary())
        self.assertIn("premature_termination", rec["failure_modes"])
        self.assertIn("task_derailment", rec["failure_modes"])
        self.assertNotIn("weak_verification", rec["failure_modes"])

    def test_tool_success_rate_safe_when_total_zero(self) -> None:
        rec = analyze_run([], [], _summary(tools={"total": 0, "success": 0}))
        self.assertEqual(rec["tools"]["success_rate"], 0.0)

    def test_handles_no_spans_no_trajectory_gracefully(self) -> None:
        rec = analyze_run([], [], _summary())
        self.assertEqual(rec["spans_seen"], 0)
        self.assertEqual(rec["trajectory_lines"], 0)
        self.assertEqual(rec["first_action_latency_ms"], 0.0)


class TestAggregate(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="bench-agg-"))
        # Create two benchmark subdirs with sample analyses
        for bench, n in [("tbench-core", 3), ("swebench-lite", 2)]:
            d = self.tmp / bench
            d.mkdir()
            for i in range(n):
                rec = {
                    "run_id": f"{bench}-{i}",
                    "model": "gpt-5.4",
                    "tokens": {"input": 1000, "output": 200, "total": 1200},
                    "cost_usd": 0.008,
                    "iterations": 3,
                    "tools": {"total": 5, "success": 5, "success_rate": 1.0},
                    "duration_ms_wall": 4000 + i * 1000,
                    "failure_modes": ["weak_verification"] if i == 0 else [],
                }
                (d / f"task-{i}.json").write_text(json.dumps(rec))

    def test_collect_records_groups_by_subdir(self) -> None:
        recs = collect_records(self.tmp)
        self.assertEqual(set(recs.keys()), {"tbench-core", "swebench-lite"})
        self.assertEqual(len(recs["tbench-core"]), 3)
        self.assertEqual(len(recs["swebench-lite"]), 2)

    def test_benchmark_summary_computes_totals(self) -> None:
        recs = collect_records(self.tmp)
        s = benchmark_summary(recs["tbench-core"])
        self.assertEqual(s["task_count"], 3)
        self.assertAlmostEqual(s["total_cost_usd"], 0.024, places=4)
        self.assertEqual(s["tool_success_count"], 3)

    def test_write_markdown_creates_report_with_all_sections(self) -> None:
        out = self.tmp / "comparison.md"
        write_markdown(collect_records(self.tmp), out, manifest={"git_sha": "abc1234"})
        text = out.read_text()
        self.assertIn("Theo Code", text)
        self.assertIn("Per-benchmark summary", text)
        self.assertIn("tbench-core", text)
        self.assertIn("swebench-lite", text)
        self.assertIn("git_sha", text)
        # Failure mode taxonomy must include `weak_verification` (count=2 — once per bench)
        self.assertIn("weak_verification", text)

    def test_write_markdown_handles_empty_input(self) -> None:
        out = self.tmp / "empty.md"
        write_markdown({}, out)
        text = out.read_text()
        self.assertIn("Per-benchmark summary", text)
        self.assertIn("Grand total", text)


if __name__ == "__main__":
    unittest.main(verbosity=2)
