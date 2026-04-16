"""Unit tests for _headless.py — the core executor module."""

import json
import math

import pytest

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _headless import (
    HeadlessResult,
    AggregatedResult,
    _parse_json_line,
    _std,
    _wilson_ci,
    estimate_cost,
)


# ---------------------------------------------------------------------------
# HeadlessResult.from_json
# ---------------------------------------------------------------------------


class TestHeadlessResultFromJson:
    def test_parses_complete_json(self):
        data = {
            "schema": "theo.headless.v1",
            "success": True,
            "summary": "Fixed the bug",
            "iterations": 5,
            "duration_ms": 12000,
            "tokens": {"input": 3000, "output": 500, "total": 3500},
            "tools": {"total": 10, "success": 8},
            "llm": {"calls": 6, "retries": 1},
            "files_edited": ["src/main.rs"],
            "model": "gpt-4o",
            "mode": "agent",
            "provider": "openai",
        }

        result = HeadlessResult.from_json(data, exit_code=0)

        assert result.success is True
        assert result.summary == "Fixed the bug"
        assert result.iterations == 5
        assert result.duration_ms == 12000
        assert result.tokens_input == 3000
        assert result.tokens_output == 500
        assert result.tokens_total == 3500
        assert result.tool_calls_total == 10
        assert result.tool_calls_success == 8
        assert result.llm_calls == 6
        assert result.llm_retries == 1
        assert result.files_edited == ["src/main.rs"]
        assert result.model == "gpt-4o"
        assert result.exit_code == 0
        assert result.error is None
        assert result.cost_usd > 0

    def test_handles_missing_fields_gracefully(self):
        data = {"success": False}
        result = HeadlessResult.from_json(data)

        assert result.success is False
        assert result.summary == ""
        assert result.iterations == 0
        assert result.tokens_input == 0
        assert result.files_edited == []

    def test_handles_files_edited_as_int(self):
        """Some versions emit files_edited as count instead of list."""
        data = {"success": True, "files_edited": 3}
        result = HeadlessResult.from_json(data)

        assert result.files_edited == []

    def test_handles_none_token_values(self):
        data = {"success": True, "tokens": {"input": None, "output": None}}
        result = HeadlessResult.from_json(data)

        assert result.tokens_input == 0
        assert result.tokens_output == 0

    def test_computes_total_from_input_output(self):
        data = {"success": True, "tokens": {"input": 100, "output": 50}}
        result = HeadlessResult.from_json(data)

        assert result.tokens_total == 150


class TestHeadlessResultFromError:
    def test_creates_failed_result(self):
        result = HeadlessResult.from_error("Timeout after 600s", exit_code=-1)

        assert result.success is False
        assert result.error == "Timeout after 600s"
        assert result.exit_code == -1
        assert result.iterations == 0


# ---------------------------------------------------------------------------
# JSON parsing
# ---------------------------------------------------------------------------


class TestParseJsonLine:
    def test_parses_last_json_line(self):
        stdout = 'Some log output\n{"schema": "theo.headless.v1", "success": true}\n'
        result = _parse_json_line(stdout)

        assert result is not None
        assert result["success"] is True

    def test_ignores_non_json_lines(self):
        stdout = "just text\nno json here\n"
        result = _parse_json_line(stdout)

        assert result is None

    def test_handles_empty_stdout(self):
        assert _parse_json_line("") is None
        assert _parse_json_line(None) is None

    def test_finds_json_among_multiple_lines(self):
        stdout = '{"bad": json\n{"success": false, "error": "timeout"}\n'
        result = _parse_json_line(stdout)

        assert result is not None
        assert result["success"] is False

    def test_prefers_last_json_line(self):
        stdout = '{"first": true}\n{"last": true}\n'
        result = _parse_json_line(stdout)

        assert result is not None
        assert result.get("last") is True


# ---------------------------------------------------------------------------
# Cost estimation
# ---------------------------------------------------------------------------


class TestEstimateCost:
    def test_known_model(self):
        cost = estimate_cost("gpt-4o", 1_000_000, 100_000)
        assert cost == pytest.approx(2.50 + 1.00, abs=0.01)

    def test_unknown_model_returns_zero(self):
        cost = estimate_cost("some-unknown-model", 1000, 1000)
        assert cost == 0.0

    def test_local_model_is_free(self):
        cost = estimate_cost("qwen-coder-30B", 500_000, 100_000)
        assert cost == 0.0

    def test_empty_model(self):
        assert estimate_cost("", 1000, 1000) == 0.0

    def test_zero_tokens(self):
        assert estimate_cost("gpt-4o", 0, 0) == 0.0

    def test_case_insensitive_matching(self):
        cost = estimate_cost("GPT-4O-mini-2026-01", 1_000_000, 0)
        assert cost > 0


# ---------------------------------------------------------------------------
# Statistics
# ---------------------------------------------------------------------------


class TestStd:
    def test_single_value(self):
        assert _std([5]) == 0.0

    def test_empty(self):
        assert _std([]) == 0.0

    def test_known_values(self):
        # std of [2, 4, 4, 4, 5, 5, 7, 9] = 2.0 (population), 2.138 (sample)
        result = _std([2, 4, 4, 4, 5, 5, 7, 9])
        assert result == pytest.approx(2.138, abs=0.01)

    def test_identical_values(self):
        assert _std([3, 3, 3, 3]) == 0.0


class TestWilsonCi:
    def test_all_success(self):
        lo, hi = _wilson_ci(10, 10)
        assert lo > 0.7
        assert hi == pytest.approx(1.0, abs=0.01)

    def test_no_success(self):
        lo, hi = _wilson_ci(0, 10)
        assert lo == pytest.approx(0.0, abs=0.01)
        assert hi < 0.3

    def test_half_success(self):
        lo, hi = _wilson_ci(5, 10)
        assert lo < 0.5
        assert hi > 0.5

    def test_zero_trials(self):
        lo, hi = _wilson_ci(0, 0)
        assert lo == 0.0
        assert hi == 0.0

    def test_small_sample(self):
        """Wilson interval should be wider for small samples."""
        lo_small, hi_small = _wilson_ci(1, 3)
        lo_large, hi_large = _wilson_ci(33, 100)
        # Small sample should have wider interval
        assert (hi_small - lo_small) > (hi_large - lo_large)
