"""Tests for the refinement cycle (autoloop)."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from autoloop.cycle import RefinementCycle, load_config, is_forbidden


def _make_config(**overrides) -> dict:
    """Create a minimal config for testing."""
    config = {
        "cycle": {
            "max_iterations": 3,
            "quality_threshold": 0.7,
            "budget_usd": 5.0,
        },
        "scope": {
            "allowed_crates": ["theo-engine-retrieval"],
            "forbidden_paths": [".claude/rules/*-allowlist.txt", "CLAUDE.md"],
        },
        "benchmarks": {
            "suites": ["smoke"],
            "nightly_suites": ["smoke", "e2e-probe"],
        },
        "thresholds": {
            "path": "docs/sota-thresholds.toml",
        },
        "output": {
            "reports_dir": "/tmp/theo-test-reports",
            "log_file": "/tmp/theo-test-reports/log.jsonl",
        },
    }
    for k, v in overrides.items():
        if "." in k:
            section, key = k.split(".", 1)
            config[section][key] = v
        else:
            config[k] = v
    return config


class TestConfig:
    def test_load_config(self):
        config = load_config()
        assert "cycle" in config
        assert "scope" in config
        assert config["cycle"]["max_iterations"] == 5

    def test_max_iterations_from_config(self):
        config = _make_config()
        cycle = RefinementCycle(config)
        assert cycle.max_iterations == 3

    def test_budget_from_config(self):
        config = _make_config()
        cycle = RefinementCycle(config)
        assert cycle.budget_usd == 5.0


class TestForbiddenPaths:
    def test_allowlist_forbidden(self):
        patterns = [".claude/rules/*-allowlist.txt"]
        assert is_forbidden(".claude/rules/unwrap-allowlist.txt", patterns) is True

    def test_claude_md_forbidden(self):
        patterns = ["CLAUDE.md"]
        assert is_forbidden("CLAUDE.md", patterns) is True

    def test_normal_file_allowed(self):
        patterns = [".claude/rules/*-allowlist.txt", "CLAUDE.md"]
        assert is_forbidden("crates/theo-engine-retrieval/src/lib.rs", patterns) is False

    def test_makefile_forbidden(self):
        config = _make_config()
        config["scope"]["forbidden_paths"].append("Makefile")
        assert is_forbidden("Makefile", config["scope"]["forbidden_paths"]) is True


class TestCycleInit:
    def test_dry_run_default(self):
        config = _make_config()
        cycle = RefinementCycle(config)
        assert cycle.apply is False

    def test_apply_mode(self):
        config = _make_config()
        cycle = RefinementCycle(config, apply=True)
        assert cycle.apply is True

    def test_initial_budget_zero(self):
        config = _make_config()
        cycle = RefinementCycle(config)
        assert cycle.spent_usd == 0.0

    def test_empty_log(self):
        config = _make_config()
        cycle = RefinementCycle(config)
        assert cycle.log == []


class TestCycleDryRun:
    def test_dry_run_produces_report(self):
        config = _make_config()
        cycle = RefinementCycle(config, apply=False)
        report = cycle.run()
        assert "schema" in report
        assert report["schema"] == "theo.refinement-cycle.v1"
        assert "iterations" in report
        assert isinstance(report["log"], list)
