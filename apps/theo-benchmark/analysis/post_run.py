"""
Post-run analysis — Phase 47 (benchmark-validation-plan).

Cross-correlates the trajectory JSONL written by `theo --headless` with
OTLP spans captured by the local collector. Produces per-task analytic
records that the aggregator (analysis/aggregate.py) consumes.

Inputs:
  - trajectory_dir: path containing `<run_id>.jsonl` files emitted by
    theo's ObservabilityListener (typically <project>/.theo/trajectories/)
  - spans_path: path to the collector's file exporter output
    (default: /var/log/otel/spans.jsonl on the droplet)

Output:
  Per task (run_id), a dict with:
    - run_id, task_id, model, provider
    - tokens.{input, output, total}
    - cost_usd
    - iterations, llm_calls, retries
    - tools.{total, success, success_rate}
    - duration_ms_wall
    - first_action_latency_ms (time from RunInitialized to first
      ToolCallDispatched, derived from spans)
    - p50_tool_dispatch_ms, p95_tool_dispatch_ms (from spans)
    - p50_llm_call_ms, p95_llm_call_ms (from spans)
    - failure_modes: list of strings detected by the runtime's
      failure_sensors module (already in the trajectory envelope)
"""

from __future__ import annotations

import json
import math
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable

# Pricing helper — same module Phase 46 introduced.
ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from pricing import compute_cost  # noqa: E402


def _percentile(values: list[float], pct: float) -> float:
    """Return the `pct` percentile (0–100) of `values` via linear interp.

    Returns 0.0 for empty list to keep aggregator math safe.
    """
    if not values:
        return 0.0
    sorted_v = sorted(values)
    if len(sorted_v) == 1:
        return float(sorted_v[0])
    k = (pct / 100.0) * (len(sorted_v) - 1)
    f = math.floor(k)
    c = math.ceil(k)
    if f == c:
        return float(sorted_v[int(k)])
    return float(sorted_v[f] + (sorted_v[c] - sorted_v[f]) * (k - f))


def load_trajectory(path: Path) -> list[dict]:
    """Load a trajectory JSONL file. Returns one dict per envelope line."""
    out: list[dict] = []
    with path.open() as fp:
        for line in fp:
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return out


def load_spans(path: Path) -> list[dict]:
    """Load OTel spans from the collector file exporter (JSONL)."""
    if not path.exists():
        return []
    spans: list[dict] = []
    with path.open() as fp:
        for line in fp:
            line = line.strip()
            if not line:
                continue
            try:
                spans.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return spans


def index_spans_by_run(spans: list[dict]) -> dict[str, list[dict]]:
    """Group spans by their `theo.run_id` attribute (or `gen_ai.agent.id`).

    Both keys are populated by the runtime's OtelExportingListener; we
    fall back from one to the other so subagent spans group with their
    parent run.
    """
    idx: dict[str, list[dict]] = defaultdict(list)
    for env in spans:
        # Collector file exporter emits OTLP/JSON per `service.signal`.
        # The exact shape varies; we just walk every span and pull the
        # run_id from attributes if present.
        for rs in env.get("resourceSpans", []) or []:
            for ss in rs.get("scopeSpans", []) or []:
                for sp in ss.get("spans", []) or []:
                    attrs = {
                        a.get("key", ""): a.get("value", {})
                        for a in sp.get("attributes", []) or []
                    }
                    rid = (
                        _attr_str(attrs.get("theo.run_id"))
                        or _attr_str(attrs.get("gen_ai.agent.id"))
                    )
                    if rid:
                        idx[rid].append({"span": sp, "attrs": attrs})
    return idx


def _attr_str(av: Any) -> str:
    """OTLP/JSON attribute values are wrapped: {"stringValue": "x"}."""
    if av is None:
        return ""
    if isinstance(av, dict):
        return av.get("stringValue") or av.get("string_value") or ""
    return str(av)


def _attr_int(av: Any) -> int:
    if av is None:
        return 0
    if isinstance(av, dict):
        for k in ("intValue", "int_value", "stringValue"):
            v = av.get(k)
            if v is not None:
                try:
                    return int(v)
                except (ValueError, TypeError):
                    pass
    try:
        return int(av)
    except (ValueError, TypeError):
        return 0


def span_duration_ms(span: dict) -> float:
    """Compute span duration from start/end timestamps (nanos)."""
    start = int(span.get("startTimeUnixNano", 0) or 0)
    end = int(span.get("endTimeUnixNano", 0) or 0)
    if start == 0 or end == 0:
        return 0.0
    return (end - start) / 1_000_000.0


def analyze_run(
    trajectory: list[dict],
    spans_for_run: Iterable[dict],
    headless_summary: dict | None = None,
) -> dict:
    """Build the analytic record for a single run.

    Args:
      trajectory: list of envelope dicts from the JSONL trajectory file
      spans_for_run: spans (already filtered to this run_id)
      headless_summary: optional `theo.headless.v2` payload (from stdout)
        — used as the source of truth for tokens/iterations/tools

    Returns:
      dict ready to be appended to the per-task report row.
    """
    summary = headless_summary or {}
    tokens = summary.get("tokens", {}) or {}
    tools = summary.get("tools", {}) or {}
    llm = summary.get("llm", {}) or {}
    model = summary.get("model", "")

    cost = compute_cost(
        int(tokens.get("input", 0) or 0),
        int(tokens.get("output", 0) or 0),
        model,
    )

    # Span-derived analytics
    spans_list = list(spans_for_run)
    tool_dispatch_ms: list[float] = []
    llm_call_ms: list[float] = []
    first_run_started_ns: int | None = None
    first_tool_dispatched_ns: int | None = None

    for entry in spans_list:
        sp = entry["span"]
        name = sp.get("name", "")
        dur = span_duration_ms(sp)
        if name == "tool.call":
            tool_dispatch_ms.append(dur)
            ts = int(sp.get("startTimeUnixNano", 0) or 0)
            if ts and (first_tool_dispatched_ns is None or ts < first_tool_dispatched_ns):
                first_tool_dispatched_ns = ts
        elif name == "llm.call":
            llm_call_ms.append(dur)
        elif name == "agent.run":
            ts = int(sp.get("startTimeUnixNano", 0) or 0)
            if ts and (first_run_started_ns is None or ts < first_run_started_ns):
                first_run_started_ns = ts

    first_action_latency_ms = 0.0
    if first_run_started_ns is not None and first_tool_dispatched_ns is not None:
        first_action_latency_ms = (first_tool_dispatched_ns - first_run_started_ns) / 1_000_000.0

    # Failure mode detection — read from trajectory summary line
    failure_modes: list[str] = []
    for env in trajectory:
        if env.get("kind") == "summary":
            payload = env.get("payload", {}) or {}
            fm = payload.get("failure_modes", {}) or {}
            for k, v in fm.items():
                if v:
                    failure_modes.append(k)
            break

    tool_total = int(tools.get("total", 0) or 0)
    tool_success = int(tools.get("success", 0) or 0)

    return {
        "run_id": summary.get("run_id", ""),
        "model": model,
        "provider": summary.get("provider", ""),
        "tokens": {
            "input": int(tokens.get("input", 0) or 0),
            "output": int(tokens.get("output", 0) or 0),
            "total": int(tokens.get("total", 0) or 0),
        },
        "cost_usd": round(cost, 6),
        "iterations": int(summary.get("iterations", 0) or 0),
        "llm_calls": int(llm.get("calls", 0) or 0),
        "retries": int(llm.get("retries", 0) or 0),
        "tools": {
            "total": tool_total,
            "success": tool_success,
            "success_rate": (
                round(tool_success / tool_total, 4) if tool_total else 0.0
            ),
        },
        "duration_ms_wall": int(summary.get("duration_ms", 0) or 0),
        "first_action_latency_ms": round(first_action_latency_ms, 2),
        "p50_tool_dispatch_ms": round(_percentile(tool_dispatch_ms, 50), 2),
        "p95_tool_dispatch_ms": round(_percentile(tool_dispatch_ms, 95), 2),
        "p50_llm_call_ms": round(_percentile(llm_call_ms, 50), 2),
        "p95_llm_call_ms": round(_percentile(llm_call_ms, 95), 2),
        "failure_modes": failure_modes,
        "spans_seen": len(spans_list),
        "trajectory_lines": len(trajectory),
    }


def main(argv: list[str] | None = None) -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Post-run trajectory + spans analysis")
    ap.add_argument("--trajectory", required=True, type=Path,
                    help="Path to a single .theo/trajectories/<run_id>.jsonl")
    ap.add_argument("--spans", default=Path("/var/log/otel/spans.jsonl"), type=Path,
                    help="Collector file exporter spans.jsonl (default: /var/log/otel/spans.jsonl)")
    ap.add_argument("--headless-json", type=Path,
                    help="Optional path to a JSON file containing the theo.headless.v2 payload")
    ap.add_argument("--output", type=Path,
                    help="Write the analysis dict to this file (else stdout)")
    args = ap.parse_args(argv)

    trajectory = load_trajectory(args.trajectory)
    all_spans = load_spans(args.spans)
    spans_idx = index_spans_by_run(all_spans)

    headless_summary = None
    if args.headless_json and args.headless_json.exists():
        headless_summary = json.loads(args.headless_json.read_text())

    # Pick spans for this trajectory's run_id (best-effort).
    run_id = (headless_summary or {}).get("run_id") or args.trajectory.stem
    spans_for_run = spans_idx.get(run_id, [])

    record = analyze_run(trajectory, spans_for_run, headless_summary)
    out_text = json.dumps(record, indent=2)
    if args.output:
        args.output.write_text(out_text)
    else:
        print(out_text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
