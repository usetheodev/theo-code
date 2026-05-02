#!/usr/bin/env python3
"""Threshold Checker — compares measured metrics against SOTA thresholds.

Reads `docs/sota-thresholds.toml` and compares dod-gate floors against
current measured values. Produces a PASS/FAIL report.

Usage:
    python e2e/threshold_checker.py                    # check all dod-gates
    python e2e/threshold_checker.py --section retrieval # check only retrieval
    python e2e/threshold_checker.py --json             # output as JSON
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ImportError:
        print("ERROR: Python 3.11+ or 'pip install tomli' required", file=sys.stderr)
        sys.exit(1)

THRESHOLDS_PATH = Path(__file__).resolve().parent.parent.parent.parent / "docs" / "sota-thresholds.toml"


def load_thresholds(path: Path | None = None) -> dict:
    """Load the SOTA thresholds TOML."""
    path = path or THRESHOLDS_PATH
    with open(path, "rb") as f:
        return tomllib.load(f)


def check_dod_gates(data: dict, section: str | None = None) -> list[dict]:
    """Check all dod-gate thresholds and return results."""
    results = []
    for sect_name, sect_values in data.items():
        if sect_name == "meta":
            continue
        if section and sect_name != section:
            continue
        for key, threshold in sect_values.items():
            if not isinstance(threshold, dict):
                continue
            if threshold.get("type") != "dod-gate":
                continue

            floor = threshold.get("floor")
            current = threshold.get("current")
            status = threshold.get("status", "")

            # Determine pass/fail
            if status in ("UNMEASURED", "unmeasured"):
                verdict = "SKIP"
                reason = "Not yet measured"
            elif current == "unmeasured":
                verdict = "SKIP"
                reason = "Not yet measured"
            elif isinstance(current, (int, float)) and isinstance(floor, (int, float)):
                if current >= floor:
                    verdict = "PASS"
                    reason = f"{current} >= {floor}"
                else:
                    verdict = "FAIL"
                    reason = f"{current} < {floor} (gap: {floor - current:.4f})"
            else:
                verdict = "SKIP"
                reason = f"Cannot compare: current={current}, floor={floor}"

            results.append({
                "section": sect_name,
                "key": key,
                "floor": floor,
                "current": current,
                "verdict": verdict,
                "reason": reason,
                "confidence": threshold.get("confidence", 0),
                "source": threshold.get("source", ""),
            })
    return results


def format_table(results: list[dict]) -> str:
    """Format results as a readable table."""
    lines = [
        "| Section | Gate | Floor | Current | Verdict | Reason |",
        "|---------|------|-------|---------|---------|--------|",
    ]
    for r in results:
        current_str = str(r["current"]) if r["current"] != "unmeasured" else "—"
        lines.append(
            f"| {r['section']} | {r['key']} | {r['floor']} | "
            f"{current_str} | {r['verdict']} | {r['reason']} |"
        )

    passed = sum(1 for r in results if r["verdict"] == "PASS")
    failed = sum(1 for r in results if r["verdict"] == "FAIL")
    skipped = sum(1 for r in results if r["verdict"] == "SKIP")
    total = len(results)

    lines.extend([
        "",
        f"**Summary:** {passed} PASS, {failed} FAIL, {skipped} SKIP (of {total} dod-gates)",
    ])

    if failed > 0:
        lines.append(f"\n**EXIT 1** — {failed} gate(s) below floor")
    else:
        lines.append(f"\n**EXIT 0** — all measured gates pass")

    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="SOTA Threshold Checker")
    parser.add_argument("--section", type=str, help="Check only this section")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    parser.add_argument("--thresholds", type=str, help="Custom thresholds TOML path")
    args = parser.parse_args()

    path = Path(args.thresholds) if args.thresholds else None
    data = load_thresholds(path)
    results = check_dod_gates(data, section=args.section)

    if args.json:
        print(json.dumps(results, indent=2))
    else:
        print(format_table(results))

    failed = sum(1 for r in results if r["verdict"] == "FAIL")
    sys.exit(1 if failed > 0 else 0)


if __name__ == "__main__":
    main()
