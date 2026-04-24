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
    _surrogate_value,
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


# ---------------------------------------------------------------------------
# Temperature CLI flag propagation (P0 bug fix validation)
# ---------------------------------------------------------------------------


class TestTemperaturePropagation:
    """Validates that temperature is passed as CLI flag, not just env var."""

    def test_temperature_flag_in_command(self):
        """--temperature must appear in the subprocess command args."""
        from unittest.mock import patch, MagicMock
        from _headless import run_headless

        with patch("_headless.subprocess.run") as mock_run, \
             patch("pathlib.Path.exists", return_value=True):
            mock_proc = MagicMock()
            mock_proc.returncode = 0
            mock_proc.stdout = '{"schema":"theo.headless.v2","success":true,"summary":"ok","iterations":1,"duration_ms":100,"tokens":{"input":10,"output":5,"total":15},"tools":{"total":1,"success":1},"llm":{"calls":1,"retries":0},"files_edited":[],"model":"test","mode":"agent","provider":"test"}'
            mock_proc.stderr = ""
            mock_run.return_value = mock_proc

            run_headless("test prompt", temperature=0.0, theo_bin=Path("/fake/theo"))

            # Verify --temperature 0.0 appears in the command
            call_args = mock_run.call_args
            cmd = call_args[0][0]  # positional arg 0 = command list
            assert "--temperature" in cmd, f"--temperature not found in command: {cmd}"
            temp_idx = cmd.index("--temperature")
            assert cmd[temp_idx + 1] == "0.0", f"temperature value wrong: {cmd[temp_idx + 1]}"

    def test_no_temperature_flag_when_none(self):
        """When temperature is None, --temperature should NOT appear in command."""
        from unittest.mock import patch, MagicMock
        from _headless import run_headless

        with patch("_headless.subprocess.run") as mock_run, \
             patch("pathlib.Path.exists", return_value=True):
            mock_proc = MagicMock()
            mock_proc.returncode = 0
            mock_proc.stdout = '{"schema":"theo.headless.v2","success":true,"summary":"ok","iterations":1,"duration_ms":100,"tokens":{"input":10,"output":5,"total":15},"tools":{"total":1,"success":1},"llm":{"calls":1,"retries":0},"files_edited":[],"model":"test","mode":"agent","provider":"test"}'
            mock_proc.stderr = ""
            mock_run.return_value = mock_proc

            run_headless("test prompt", temperature=None, theo_bin=Path("/fake/theo"))

            cmd = mock_run.call_args[0][0]
            assert "--temperature" not in cmd, f"--temperature should not be in command when None: {cmd}"

    def test_headless_v2_schema_parsed(self):
        """HeadlessResult should parse the v2 schema with environment block."""
        data = {
            "schema": "theo.headless.v2",
            "success": True,
            "summary": "done",
            "iterations": 3,
            "duration_ms": 5000,
            "tokens": {"input": 1000, "output": 200, "total": 1200},
            "tools": {"total": 5, "success": 5},
            "llm": {"calls": 3, "retries": 0},
            "files_edited": [],
            "model": "qwen3-30B",
            "mode": "agent",
            "provider": "local",
            "environment": {
                "temperature_actual": 0.0,
                "theo_version": "0.1.0",
            },
        }
        result = HeadlessResult.from_json(data, exit_code=0)
        assert result.success is True
        assert result.model == "qwen3-30B"
        # Environment block is in raw_json
        assert result.raw_json["environment"]["temperature_actual"] == 0.0


# ---------------------------------------------------------------------------
# V4 RunReport parsing
# ---------------------------------------------------------------------------


def _base_v3_json(**overrides) -> dict:
    """Minimal v3 JSON fixture — no report field."""
    data = {
        "success": True,
        "summary": "Resolved the issue",
        "iterations": 4,
        "duration_ms": 8000,
        "tokens": {"input": 2000, "output": 400, "total": 2400},
        "tools": {"total": 7, "success": 6},
        "llm": {"calls": 4, "retries": 0},
        "model": "qwen3-30B",
        "mode": "agent",
        "provider": "local",
        "files_edited": ["src/lib.rs"],
    }
    data.update(overrides)
    return data


class TestV4ReportParsing:
    """Tests for v4 RunReport parsing in HeadlessResult.from_json."""

    def test_v3_json_without_report_uses_defaults(self):
        data = _base_v3_json()
        result = HeadlessResult.from_json(data)

        # Extended token metrics default to 0
        assert result.cache_read_tokens == 0
        assert result.reasoning_tokens == 0
        assert result.cache_hit_rate == 0.0
        assert result.tokens_per_successful_edit == 0.0

        # Loop metrics default
        assert result.convergence_rate == 0.0
        assert result.budget_utilization_iterations_pct == 0.0
        assert result.phase_distribution == {}
        assert result.evolution_attempts == 0

        # Tool breakdown default
        assert result.tool_breakdown == []

        # Context health default
        assert result.context_avg_size_tokens == 0.0
        assert result.context_max_size_tokens == 0

        # Memory metrics default
        assert result.memory_episodes_injected == 0
        assert result.memory_hypotheses_formed == 0

        # Subagent metrics default
        assert result.subagent_spawned == 0
        assert result.subagent_success_rate == 0.0

        # Error taxonomy default
        assert result.error_total == 0

        # Surrogate metrics default
        assert result.doom_loop_frequency == 0.0
        assert result.llm_efficiency == 0.0

        # Integrity defaults
        assert result.trajectory_complete is True
        assert result.trajectory_confidence == 1.0

        # Error class default
        assert result.error_class == ""

    def test_v4_parses_token_metrics(self):
        data = _base_v3_json(report={
            "token_metrics": {
                "cache_read_tokens": 1500,
                "cache_write_tokens": 300,
                "reasoning_tokens": 200,
                "cache_hit_rate": 0.75,
                "tokens_per_successful_edit": 1200.5,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.cache_read_tokens == 1500
        assert result.cache_write_tokens == 300
        assert result.reasoning_tokens == 200
        assert result.cache_hit_rate == 0.75
        assert result.tokens_per_successful_edit == 1200.5

    def test_v4_parses_loop_metrics(self):
        data = _base_v3_json(report={
            "loop_metrics": {
                "convergence_rate": 0.85,
                "budget_utilization": {
                    "iterations_pct": 0.6,
                    "tokens_pct": 0.4,
                    "time_pct": 0.3,
                },
                "phase_distribution": {"Planning": 2, "Executing": 5, "Evaluating": 1},
                "evolution_attempts": 3,
                "evolution_success": True,
                "done_blocked_count": 1,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.convergence_rate == 0.85
        assert result.budget_utilization_iterations_pct == 0.6
        assert result.budget_utilization_tokens_pct == 0.4
        assert result.budget_utilization_time_pct == 0.3
        assert result.phase_distribution == {"Planning": 2, "Executing": 5, "Evaluating": 1}
        assert result.evolution_attempts == 3
        assert result.evolution_success is True
        assert result.done_blocked_count == 1

    def test_v4_parses_tool_breakdown(self):
        breakdown = [
            {"tool_name": "read_file", "call_count": 5, "success_count": 5, "avg_duration_ms": 12.0},
            {"tool_name": "edit_file", "call_count": 3, "success_count": 2, "avg_duration_ms": 45.0},
        ]
        data = _base_v3_json(report={"tool_breakdown": breakdown})
        result = HeadlessResult.from_json(data)

        assert len(result.tool_breakdown) == 2
        assert result.tool_breakdown[0]["tool_name"] == "read_file"
        assert result.tool_breakdown[0]["call_count"] == 5
        assert result.tool_breakdown[1]["tool_name"] == "edit_file"
        assert result.tool_breakdown[1]["success_count"] == 2

    def test_v4_parses_context_health(self):
        data = _base_v3_json(report={
            "context_health": {
                "avg_context_size_tokens": 3500.0,
                "max_context_size_tokens": 8000,
                "context_growth_rate": 1.2,
                "compaction_count": 2,
                "compaction_savings_ratio": 0.35,
                "refetch_rate": 0.1,
                "action_repetition_rate": 0.05,
                "usefulness_avg": 0.82,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.context_avg_size_tokens == 3500.0
        assert result.context_max_size_tokens == 8000
        assert result.context_growth_rate == 1.2
        assert result.context_compaction_count == 2
        assert result.context_compaction_savings_ratio == 0.35
        assert result.context_refetch_rate == 0.1
        assert result.context_action_repetition_rate == 0.05
        assert result.context_usefulness_avg == 0.82

    def test_v4_parses_memory_metrics(self):
        data = _base_v3_json(report={
            "memory_metrics": {
                "episodes_injected": 3,
                "episodes_created": 2,
                "hypotheses_formed": 5,
                "hypotheses_invalidated": 1,
                "hypotheses_active": 4,
                "constraints_learned": 7,
                "failure_fingerprints_new": 2,
                "failure_fingerprints_recurrent": 1,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.memory_episodes_injected == 3
        assert result.memory_episodes_created == 2
        assert result.memory_hypotheses_formed == 5
        assert result.memory_hypotheses_invalidated == 1
        assert result.memory_hypotheses_active == 4
        assert result.memory_constraints_learned == 7
        assert result.memory_failure_fingerprints_new == 2
        assert result.memory_failure_fingerprints_recurrent == 1

    def test_v4_parses_subagent_metrics(self):
        data = _base_v3_json(report={
            "subagent_metrics": {
                "spawned": 4,
                "succeeded": 3,
                "failed": 1,
                "avg_duration_ms": 2500.0,
                "success_rate": 0.75,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.subagent_spawned == 4
        assert result.subagent_succeeded == 3
        assert result.subagent_failed == 1
        assert result.subagent_avg_duration_ms == 2500.0
        assert result.subagent_success_rate == 0.75

    def test_v4_parses_error_taxonomy(self):
        data = _base_v3_json(report={
            "error_taxonomy": {
                "total_errors": 6,
                "network_errors": 1,
                "llm_errors": 2,
                "tool_errors": 1,
                "sandbox_errors": 0,
                "budget_errors": 1,
                "validation_errors": 1,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.error_total == 6
        assert result.error_network == 1
        assert result.error_llm == 2
        assert result.error_tool == 1
        assert result.error_sandbox == 0
        assert result.error_budget == 1
        assert result.error_validation == 1

    def test_v4_parses_surrogate_metrics(self):
        data = _base_v3_json(report={
            "surrogate_metrics": {
                "doom_loop_frequency": {"value": 0.1, "confidence": 0.9, "method": "heuristic"},
                "llm_efficiency": {"value": 0.85, "confidence": 1.0, "method": "ratio"},
                "context_waste_ratio": {"value": 0.05, "confidence": 0.8, "method": "measured"},
                "hypothesis_churn_rate": {"value": 0.2, "confidence": 0.7, "method": "count"},
                "time_to_first_tool_ms": {"value": 450.0, "confidence": 1.0, "method": "timer"},
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.doom_loop_frequency == 0.1
        assert result.llm_efficiency == 0.85
        assert result.context_waste_ratio == 0.05
        assert result.hypothesis_churn_rate == 0.2
        assert result.time_to_first_tool_ms == 450.0

    def test_v4_parses_integrity(self):
        data = _base_v3_json(report={
            "integrity": {
                "complete": False,
                "confidence": 0.6,
            }
        })
        result = HeadlessResult.from_json(data)

        assert result.trajectory_complete is False
        assert result.trajectory_confidence == 0.6

    def test_v4_partial_report_uses_defaults(self):
        """A report with only token_metrics should leave other sections at defaults."""
        data = _base_v3_json(report={
            "token_metrics": {
                "cache_read_tokens": 500,
                "reasoning_tokens": 100,
            }
        })
        result = HeadlessResult.from_json(data)

        # Provided section is parsed
        assert result.cache_read_tokens == 500
        assert result.reasoning_tokens == 100

        # Missing sections use defaults
        assert result.convergence_rate == 0.0
        assert result.phase_distribution == {}
        assert result.tool_breakdown == []
        assert result.context_avg_size_tokens == 0.0
        assert result.memory_episodes_injected == 0
        assert result.subagent_spawned == 0
        assert result.error_total == 0
        assert result.doom_loop_frequency == 0.0
        assert result.trajectory_complete is True
        assert result.trajectory_confidence == 1.0

    def test_v4_parses_error_class(self):
        data = _base_v3_json(success=False, error_class="timeout")
        result = HeadlessResult.from_json(data)

        assert result.error_class == "timeout"

    def test_surrogate_value_extracts_value(self):
        parent = {"metric_a": {"value": 0.5, "confidence": 1.0, "method": "direct"}}
        assert _surrogate_value(parent, "metric_a") == 0.5

    def test_surrogate_value_missing_key(self):
        parent = {"metric_a": {"value": 0.5}}
        assert _surrogate_value(parent, "nonexistent") == 0.0

    def test_surrogate_value_non_dict(self):
        parent = {"metric_a": 42}
        assert _surrogate_value(parent, "metric_a") == 0.0
