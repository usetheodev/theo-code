"""Canonical benchmark schemas for Theo Code validation pipeline."""

import json
from pathlib import Path

_SCHEMA_DIR = Path(__file__).parent
_BENCHMARK_RUN_SCHEMA = None


def _load_schema():
    global _BENCHMARK_RUN_SCHEMA
    if _BENCHMARK_RUN_SCHEMA is None:
        schema_path = _SCHEMA_DIR / "benchmark-run.schema.json"
        with open(schema_path) as f:
            _BENCHMARK_RUN_SCHEMA = json.load(f)
    return _BENCHMARK_RUN_SCHEMA


def get_benchmark_run_schema() -> dict:
    """Return the benchmark-run JSON Schema as a dict."""
    return _load_schema()


def validate_benchmark_run(data: dict) -> list[str]:
    """Validate a benchmark run dict against the schema.

    Returns a list of error messages (empty = valid).
    Uses a simple field-presence check; for full JSON Schema
    validation install jsonschema and use validate_strict().
    """
    schema = _load_schema()
    errors = []

    required = schema.get("required", [])
    for field in required:
        if field not in data:
            errors.append(f"Missing required field: {field}")

    if "schema_version" in data and data["schema_version"] != "theo.benchmark-run.v1":
        errors.append(
            f"Invalid schema_version: {data['schema_version']} "
            f"(expected 'theo.benchmark-run.v1')"
        )

    if "tokens" in data:
        tok = data["tokens"]
        for sub in ("input", "output", "total"):
            if sub not in tok:
                errors.append(f"Missing tokens.{sub}")

    return errors


def validate_strict(data: dict) -> list[str]:
    """Full JSON Schema validation (requires jsonschema package)."""
    try:
        import jsonschema
    except ImportError:
        return ["jsonschema package not installed; falling back to validate_benchmark_run()"] + validate_benchmark_run(data)

    schema = _load_schema()
    validator = jsonschema.Draft202012Validator(schema)
    return [e.message for e in validator.iter_errors(data)]
