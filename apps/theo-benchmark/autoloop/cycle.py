#!/usr/bin/env python3
"""Refinement Cycle — iterative improvement with keep/discard pattern.

Implements the self-evolution loop (+4.8 SWE-Bench, Tsinghua ablation):
  1. Run benchmark → baseline metrics
  2. Read threshold comparison → identify worst gap
  3. Generate hypothesis
  4. Apply bounded change (human-gated)
  5. Re-run benchmark → new metrics
  6. Keep if improved, discard if regressed
  7. Repeat until max_iterations or budget exhausted

Usage:
    python autoloop/cycle.py                 # dry-run (no changes applied)
    python autoloop/cycle.py --apply         # apply changes (still human-gated)
    python autoloop/cycle.py --nightly       # use nightly suites (more expensive)
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ImportError:
        print("ERROR: Python 3.11+ or 'pip install tomli' required", file=sys.stderr)
        sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = Path(__file__).resolve().parent / "config.toml"


def load_config(path: Path | None = None) -> dict:
    """Load cycle configuration."""
    path = path or CONFIG_PATH
    with open(path, "rb") as f:
        return tomllib.load(f)


def is_forbidden(filepath: str, forbidden_patterns: list[str]) -> bool:
    """Check if a file path matches any forbidden pattern."""
    import fnmatch
    for pattern in forbidden_patterns:
        if fnmatch.fnmatch(filepath, pattern):
            return True
    return False


class RefinementCycle:
    """Main refinement cycle orchestrator."""

    def __init__(self, config: dict, *, apply: bool = False, nightly: bool = False):
        self.config = config
        self.apply = apply
        self.nightly = nightly
        self.max_iterations = config["cycle"]["max_iterations"]
        self.budget_usd = config["cycle"]["budget_usd"]
        self.quality_threshold = config["cycle"]["quality_threshold"]
        self.allowed_crates = config["scope"]["allowed_crates"]
        self.forbidden_paths = config["scope"]["forbidden_paths"]
        self.spent_usd = 0.0
        self.log: list[dict] = []

    def run(self) -> dict:
        """Execute the refinement cycle."""
        print(f"Refinement Cycle — max {self.max_iterations} iterations, budget ${self.budget_usd}")
        print(f"  Allowed crates: {', '.join(self.allowed_crates)}")
        print(f"  Apply mode: {self.apply}")
        print()

        for i in range(1, self.max_iterations + 1):
            if self.spent_usd >= self.budget_usd:
                print(f"\n  Budget exhausted (${self.spent_usd:.2f} >= ${self.budget_usd})")
                break

            print(f"--- Iteration {i}/{self.max_iterations} ---")
            iteration_result = self._run_iteration(i)
            self.log.append(iteration_result)

            if iteration_result["verdict"] == "STOP":
                print(f"  All gates passing. Stopping.")
                break

        return self._produce_report()

    def _run_iteration(self, iteration: int) -> dict:
        """Run a single iteration of the cycle."""
        # Step 1: Identify worst gap
        gap = self._identify_worst_gap()
        if gap is None:
            return {"iteration": iteration, "verdict": "STOP", "reason": "All gates pass"}

        print(f"  Worst gap: {gap['section']}.{gap['key']} — {gap['reason']}")

        # Step 2: Generate hypothesis
        hypothesis = self._generate_hypothesis(gap)
        print(f"  Hypothesis: {hypothesis}")

        if not self.apply:
            print(f"  [DRY-RUN] Would apply change. Use --apply to enable.")
            return {
                "iteration": iteration,
                "verdict": "DRY_RUN",
                "gap": gap,
                "hypothesis": hypothesis,
            }

        # Step 3: Human gate
        print(f"\n  HUMAN GATE: Review the hypothesis above.")
        print(f"  The change will target: {', '.join(self.allowed_crates)}")
        response = input("  Proceed? [y/N] ").strip().lower()
        if response != "y":
            return {
                "iteration": iteration,
                "verdict": "HUMAN_REJECTED",
                "gap": gap,
                "hypothesis": hypothesis,
            }

        # Step 4-6: Apply, test, keep/discard would go here
        # For now, return placeholder — actual implementation requires
        # running theo --headless and cargo test
        return {
            "iteration": iteration,
            "verdict": "PENDING_IMPLEMENTATION",
            "gap": gap,
            "hypothesis": hypothesis,
            "note": "Apply/test/keep-discard requires running benchmarks",
        }

    def _identify_worst_gap(self) -> dict | None:
        """Find the dod-gate with the largest gap below floor."""
        # Import threshold checker
        sys.path.insert(0, str(ROOT))
        from e2e.threshold_checker import load_thresholds, check_dod_gates

        data = load_thresholds()
        results = check_dod_gates(data)

        # Find the FAIL with largest gap
        worst = None
        worst_gap = 0.0
        for r in results:
            if r["verdict"] == "FAIL":
                current = r.get("current", 0)
                floor = r.get("floor", 0)
                if isinstance(current, (int, float)) and isinstance(floor, (int, float)):
                    gap = floor - current
                    if gap > worst_gap:
                        worst_gap = gap
                        worst = r
        return worst

    def _generate_hypothesis(self, gap: dict) -> str:
        """Generate a hypothesis for improving the worst gap."""
        section = gap["section"]
        key = gap["key"]
        current = gap.get("current", "?")
        floor = gap.get("floor", "?")

        if section == "retrieval":
            if "recall" in key:
                return (
                    f"Improve {key} from {current} to >={floor} by adjusting "
                    f"RRF k-constant or BM25F field weights in theo-engine-retrieval"
                )
            elif key == "mrr":
                return (
                    f"Improve MRR from {current} to >={floor} by tuning "
                    f"graph enrichment hop depth or noise filter thresholds"
                )
            elif key == "depcov":
                return (
                    f"Improve DepCov from {current} to >={floor} by expanding "
                    f"2-hop import edge coverage in graph enrichment"
                )
        elif section == "smoke":
            return (
                f"Improve smoke pass rate from {current} to >={floor} by "
                f"fixing failing scenarios in agent runtime"
            )

        return f"Improve {section}.{key} from {current} to >={floor}"

    def _produce_report(self) -> dict:
        """Produce the final refinement cycle report."""
        return {
            "schema": "theo.refinement-cycle.v1",
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "config": {
                "max_iterations": self.max_iterations,
                "budget_usd": self.budget_usd,
                "quality_threshold": self.quality_threshold,
            },
            "iterations": len(self.log),
            "spent_usd": self.spent_usd,
            "log": self.log,
        }


def main():
    parser = argparse.ArgumentParser(description="Refinement Cycle for Theo Code")
    parser.add_argument("--apply", action="store_true", help="Apply changes (human-gated)")
    parser.add_argument("--nightly", action="store_true", help="Use nightly benchmark suites")
    parser.add_argument("--config", type=str, help="Custom config path")
    parser.add_argument("--output", type=str, help="Custom report output path")
    args = parser.parse_args()

    config_path = Path(args.config) if args.config else None
    config = load_config(config_path)

    cycle = RefinementCycle(config, apply=args.apply, nightly=args.nightly)
    report = cycle.run()

    # Save report
    reports_dir = ROOT / "reports"
    reports_dir.mkdir(parents=True, exist_ok=True)
    report_path = args.output or str(reports_dir / f"refinement-{int(time.time())}.json")
    with open(report_path, "w") as f:
        json.dump(report, f, indent=2)

    print(f"\nReport: {report_path}")
    print(f"Iterations: {report['iterations']}, Spent: ${report['spent_usd']:.2f}")


if __name__ == "__main__":
    main()
