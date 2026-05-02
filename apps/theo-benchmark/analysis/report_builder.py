"""
Report Builder — benchmark-sota-metrics-plan.

Orchestrator that calls every analysis module and produces a consolidated
report dict. Each module call is wrapped in try/except so a single module
failure does not block the entire report.

Also provides report_to_markdown() to render a GitHub-renderable Markdown
report from the consolidated dict.
"""

from __future__ import annotations

import math
import sys
from dataclasses import asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .stats_utils import safe_div

# ---------------------------------------------------------------------------
# Wilson CI (same implementation as _headless.py, imported here for
# self-containment within the analysis package)
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


def _wilson_ci(successes: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """Wilson score interval for binomial proportion (95% CI)."""
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
# Result conversion
# ---------------------------------------------------------------------------


def _to_dicts(results: list) -> list[dict]:
    """Convert a list of results to dicts.

    Handles both plain dicts and dataclass instances (HeadlessResult).
    """
    out: list[dict] = []
    for r in results:
        if isinstance(r, dict):
            out.append(r)
        else:
            try:
                out.append(asdict(r))
            except Exception:
                # Last resort: use __dict__ if available
                out.append(vars(r) if hasattr(r, "__dict__") else {})
    return out


# ---------------------------------------------------------------------------
# Module runner helper
# ---------------------------------------------------------------------------


def _run_module(label: str, fn, *args, **kwargs) -> dict:
    """Call fn(*args, **kwargs), returning {"error": str} on failure."""
    try:
        result = fn(*args, **kwargs)
        if not isinstance(result, dict):
            return {"error": f"{label} returned {type(result).__name__}, expected dict"}
        return result
    except Exception as e:
        return {"error": f"{label}: {e}"}


# ---------------------------------------------------------------------------
# Build report
# ---------------------------------------------------------------------------


def build_report(
    results: list,
    benchmark_name: str,
    manifest: dict | None = None,
) -> dict:
    """Build a consolidated benchmark report from raw results.

    Args:
        results: list of HeadlessResult instances or dicts.
        benchmark_name: name of the benchmark (e.g. "tbench", "swebench-lite").
        manifest: optional provenance/config dict to embed in the report.

    Returns:
        Consolidated dict with all analysis sections. Each section key
        maps to the module's output dict, or {"error": str} if that
        module failed.
    """
    dicts = _to_dicts(results)

    if not dicts:
        return {
            "benchmark": benchmark_name,
            "manifest": manifest or {},
            "n_tasks": 0,
            "pass_rate": 0.0,
            "ci_95": [0.0, 0.0],
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "sections": {},
        }

    # --- Pass rate with Wilson CI ---
    successes = sum(1 for r in dicts if r.get("success", False))
    n = len(dicts)
    pass_rate = safe_div(successes, n)
    ci_lo, ci_hi = _wilson_ci(successes, n)

    # --- Call each analysis module ---
    sections: dict[str, Any] = {}

    # 1. Context health
    try:
        from . import context_health
        sections["context_health"] = _run_module(
            "context_health",
            context_health.analyze_context_health,
            dicts,
        )
    except ImportError as e:
        sections["context_health"] = {"error": f"import failed: {e}"}

    # 2. Tool analysis
    try:
        from . import tool_analysis
        sections["tool_analysis"] = _run_module(
            "tool_analysis",
            tool_analysis.analyze_tools,
            dicts,
        )
    except ImportError as e:
        sections["tool_analysis"] = {"error": f"import failed: {e}"}

    # 3. Loop analysis
    try:
        from . import loop_analysis
        sections["loop_analysis"] = _run_module(
            "loop_analysis",
            loop_analysis.analyze_loop,
            dicts,
        )
    except ImportError as e:
        sections["loop_analysis"] = {"error": f"import failed: {e}"}

    # 4. Memory analysis
    try:
        from . import memory_analysis
        sections["memory_analysis"] = _run_module(
            "memory_analysis",
            memory_analysis.analyze_memory,
            dicts,
        )
    except ImportError as e:
        sections["memory_analysis"] = {"error": f"import failed: {e}"}

    # 5. Error analysis
    try:
        from . import error_analysis
        sections["error_analysis"] = _run_module(
            "error_analysis",
            error_analysis.analyze_errors,
            dicts,
        )
    except ImportError as e:
        sections["error_analysis"] = {"error": f"import failed: {e}"}

    # 6. Cost analysis
    try:
        from . import cost_analysis
        sections["cost_analysis"] = _run_module(
            "cost_analysis",
            cost_analysis.analyze_cost,
            dicts,
        )
    except ImportError as e:
        sections["cost_analysis"] = {"error": f"import failed: {e}"}

    # 7. Latency analysis
    try:
        from . import latency_analysis
        sections["latency_analysis"] = _run_module(
            "latency_analysis",
            latency_analysis.analyze_latency,
            dicts,
        )
    except ImportError as e:
        sections["latency_analysis"] = {"error": f"import failed: {e}"}

    # 8. Subagent analysis
    try:
        from . import subagent_analysis
        sections["subagent_analysis"] = _run_module(
            "subagent_analysis",
            subagent_analysis.analyze_subagents,
            dicts,
        )
    except ImportError as e:
        sections["subagent_analysis"] = {"error": f"import failed: {e}"}

    # 9. Derived analysis
    try:
        from . import derived_analysis
        sections["derived_analysis"] = _run_module(
            "derived_analysis",
            derived_analysis.analyze_derived,
            dicts,
        )
    except ImportError as e:
        sections["derived_analysis"] = {"error": f"import failed: {e}"}

    # 10. Prompt analysis
    try:
        from . import prompt_analysis
        sections["prompt_analysis"] = _run_module(
            "prompt_analysis",
            prompt_analysis.analyze_prompts,
            dicts,
        )
    except ImportError as e:
        sections["prompt_analysis"] = {"error": f"import failed: {e}"}

    # 11. Phase cost analysis
    try:
        from . import phase_cost_analysis
        sections["phase_cost_analysis"] = _run_module(
            "phase_cost_analysis",
            phase_cost_analysis.analyze_phase_cost,
            dicts,
        )
    except ImportError as e:
        sections["phase_cost_analysis"] = {"error": f"import failed: {e}"}

    return {
        "benchmark": benchmark_name,
        "manifest": manifest or {},
        "n_tasks": n,
        "n_passed": successes,
        "pass_rate": round(pass_rate, 4),
        "ci_95": [round(ci_lo, 4), round(ci_hi, 4)],
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "sections": sections,
    }


# ---------------------------------------------------------------------------
# Markdown renderer
# ---------------------------------------------------------------------------


def report_to_markdown(report: dict) -> str:
    """Render a consolidated report dict as a GitHub-renderable Markdown string.

    Args:
        report: dict produced by build_report().

    Returns:
        Markdown string.
    """
    lines: list[str] = []

    benchmark = report.get("benchmark", "unknown")
    lines.append(f"# Benchmark Report: {benchmark}")
    lines.append("")
    lines.append(f"Generated: `{report.get('generated_at', 'N/A')}`")
    lines.append("")

    # Manifest
    manifest = report.get("manifest") or {}
    if manifest:
        lines.append("## Provenance")
        lines.append("")
        for k, v in manifest.items():
            lines.append(f"- **{k}**: `{v}`")
        lines.append("")

    # Headline stats
    n = report.get("n_tasks", 0)
    passed = report.get("n_passed", 0)
    rate = report.get("pass_rate", 0.0)
    ci = report.get("ci_95", [0.0, 0.0])
    lines.append("## Summary")
    lines.append("")
    lines.append(f"| Metric | Value |")
    lines.append(f"|---|---|")
    lines.append(f"| Tasks | {n} |")
    lines.append(f"| Passed | {passed} |")
    lines.append(f"| Pass rate | {rate * 100:.1f}% |")
    lines.append(f"| 95% CI (Wilson) | [{ci[0] * 100:.1f}%, {ci[1] * 100:.1f}%] |")
    lines.append("")

    # Sections
    sections = report.get("sections") or {}
    for section_name, section_data in sections.items():
        lines.append(f"## {_section_title(section_name)}")
        lines.append("")

        if isinstance(section_data, dict) and "error" in section_data:
            lines.append(f"> **Module error**: `{section_data['error']}`")
            lines.append("")
            continue

        _render_section(lines, section_data)
        lines.append("")

    return "\n".join(lines) + "\n"


def _section_title(name: str) -> str:
    """Convert snake_case section name to Title Case."""
    return name.replace("_", " ").title()


def _render_section(lines: list[str], data: Any, depth: int = 0) -> None:
    """Recursively render a section dict as Markdown.

    - Top-level dicts with string keys become sub-sections or tables.
    - Lists become bullet lists.
    - Scalars are rendered inline.
    """
    if isinstance(data, dict):
        # Check if this is a flat dict (all scalar values) -> render as table
        if data and all(_is_scalar(v) for v in data.values()):
            lines.append("| Key | Value |")
            lines.append("|---|---|")
            for k, v in data.items():
                lines.append(f"| {k} | {_fmt_value(v)} |")
            return

        # Mixed dict: render each key
        for k, v in data.items():
            if isinstance(v, dict):
                lines.append(f"### {k}")
                lines.append("")
                _render_section(lines, v, depth + 1)
                lines.append("")
            elif isinstance(v, list):
                lines.append(f"**{k}**:")
                lines.append("")
                _render_list(lines, v)
                lines.append("")
            else:
                lines.append(f"- **{k}**: {_fmt_value(v)}")
    elif isinstance(data, list):
        _render_list(lines, data)
    else:
        lines.append(str(data))


def _render_list(lines: list[str], items: list) -> None:
    """Render a list as Markdown bullets."""
    if not items:
        lines.append("(none)")
        return
    for item in items:
        if isinstance(item, dict):
            parts = [f"{k}={_fmt_value(v)}" for k, v in item.items()]
            lines.append(f"- {', '.join(parts)}")
        elif isinstance(item, (list, tuple)):
            lines.append(f"- {item}")
        else:
            lines.append(f"- {_fmt_value(item)}")


def _is_scalar(v: Any) -> bool:
    return isinstance(v, (str, int, float, bool, type(None)))


def _fmt_value(v: Any) -> str:
    """Format a scalar value for Markdown display."""
    if isinstance(v, float):
        if abs(v) < 0.0001 and v != 0.0:
            return f"{v:.6f}"
        return f"{v:.4f}" if v != int(v) else f"{int(v)}"
    if isinstance(v, bool):
        return "Yes" if v else "No"
    if v is None:
        return "N/A"
    return str(v)
