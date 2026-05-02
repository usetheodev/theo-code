"""Tests for the canonical benchmark-run schema."""

import uuid
from datetime import datetime, timezone

import pytest

from schemas import get_benchmark_run_schema, validate_benchmark_run


def _make_valid_run(**overrides) -> dict:
    """Create a minimal valid benchmark run dict."""
    base = {
        "schema_version": "theo.benchmark-run.v1",
        "run_id": str(uuid.uuid4()),
        "model_id": "gpt-4o",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "theo_sha": "abc1234",
        "task_id": "01-read-answer",
        "task_category": "smoke-read",
        "pass": True,
        "duration_ms": 5000,
        "tokens": {"input": 1000, "output": 200, "total": 1200},
        "cost_usd": 0.01,
    }
    base.update(overrides)
    return base


class TestSchemaLoads:
    def test_schema_is_dict(self):
        schema = get_benchmark_run_schema()
        assert isinstance(schema, dict)

    def test_schema_has_required_fields(self):
        schema = get_benchmark_run_schema()
        assert "required" in schema
        required = schema["required"]
        assert "run_id" in required
        assert "schema_version" in required
        assert "model_id" in required
        assert "task_id" in required

    def test_schema_version_is_v1(self):
        schema = get_benchmark_run_schema()
        assert schema["properties"]["schema_version"]["const"] == "theo.benchmark-run.v1"


class TestValidation:
    def test_valid_run_passes(self):
        errors = validate_benchmark_run(_make_valid_run())
        assert errors == []

    def test_missing_run_id_fails(self):
        run = _make_valid_run()
        del run["run_id"]
        errors = validate_benchmark_run(run)
        assert any("run_id" in e for e in errors)

    def test_missing_schema_version_fails(self):
        run = _make_valid_run()
        del run["schema_version"]
        errors = validate_benchmark_run(run)
        assert any("schema_version" in e for e in errors)

    def test_wrong_schema_version_fails(self):
        run = _make_valid_run(schema_version="theo.benchmark-run.v99")
        errors = validate_benchmark_run(run)
        assert any("schema_version" in e for e in errors)

    def test_missing_tokens_subfield_fails(self):
        run = _make_valid_run(tokens={"input": 100, "output": 50})
        errors = validate_benchmark_run(run)
        assert any("tokens.total" in e for e in errors)

    def test_all_required_fields_checked(self):
        errors = validate_benchmark_run({})
        schema = get_benchmark_run_schema()
        required = schema["required"]
        assert len(errors) >= len(required)

    def test_optional_fields_accepted(self):
        run = _make_valid_run(
            model_version="2026-04-29",
            iterations=12,
            context_bytes=50000,
            benchmark_suite="smoke",
            subtask_results=[{"id": "s1", "pass": True}],
        )
        errors = validate_benchmark_run(run)
        assert errors == []
