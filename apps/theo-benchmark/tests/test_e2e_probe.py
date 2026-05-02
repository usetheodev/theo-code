"""Tests for the E2E probe runner."""

import pytest
from pathlib import Path

# Add parent to path for imports
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from e2e.probe_runner import load_probes, _eval_success, _skip_result


class TestProbeLoading:
    def test_loads_cli_probes(self):
        probes = load_probes(suite="cli")
        assert len(probes) >= 3  # help, version, init at minimum
        assert all("id" in p for p in probes)

    def test_loads_all_probes(self):
        probes = load_probes()
        assert len(probes) >= 5  # cli + tool + provider

    def test_probe_has_required_fields(self):
        probes = load_probes()
        for p in probes:
            assert "id" in p, f"Probe missing 'id': {p}"
            assert "command" in p, f"Probe {p.get('id', '?')} missing 'command'"
            assert "success_check" in p, f"Probe {p.get('id', '?')} missing 'success_check'"

    def test_filter_by_suite(self):
        cli_probes = load_probes(suite="cli")
        tool_probes = load_probes(suite="tool")
        all_probes = load_probes()
        assert len(cli_probes) + len(tool_probes) <= len(all_probes)


class TestSuccessEval:
    def test_exit_code_zero(self):
        result = {"exit_code": 0, "stdout": "", "stderr": ""}
        assert _eval_success("exit_code == 0", result) is True

    def test_exit_code_nonzero(self):
        result = {"exit_code": 1, "stdout": "", "stderr": "error"}
        assert _eval_success("exit_code == 0", result) is False

    def test_stdout_contains(self):
        result = {"exit_code": 0, "stdout": "Usage: theo [OPTIONS]", "stderr": ""}
        assert _eval_success("'Usage' in stdout", result) is True

    def test_result_dict_access(self):
        result = {"exit_code": 0, "stdout": "", "stderr": "", "success": True}
        assert _eval_success("result.get('success', False)", result) is True

    def test_malformed_check_returns_false(self):
        result = {"exit_code": 0}
        assert _eval_success("this is not valid python", result) is False


class TestSkipResult:
    def test_skip_has_schema(self):
        probe = {"id": "test-probe", "category": "cli-subcommand"}
        result = _skip_result(probe, "test-uuid", reason="no LLM")
        assert result["schema_version"] == "theo.benchmark-run.v1"
        assert result["pass"] is False
        assert result["metadata"]["skipped"] is True
        assert result["metadata"]["skip_reason"] == "no LLM"
