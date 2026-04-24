# Headless JSON Schema

> Phase 60 (`docs/plans/headless-error-classification-plan.md`) — schema
> v3 with `error_class` field.

The `theo --headless` command emits ONE JSON object per task on stdout
(plus optional stderr logs). This document describes the canonical
schema consumers should parse against.

## Current version: `theo.headless.v3`

```json
{
  "schema": "theo.headless.v3",
  "success": true,
  "error_class": "solved",
  "summary": "Implemented function foo, all tests pass",
  "iterations": 7,
  "duration_ms": 12450,
  "tokens": {
    "input": 12500,
    "output": 850,
    "total": 13350
  },
  "tools": {
    "total": 14,
    "success": 14
  },
  "llm": {
    "calls": 7,
    "retries": 0
  },
  "files_edited": ["src/foo.rs", "src/foo_test.rs"],
  "model": "gpt-5.4",
  "mode": "agent",
  "provider": "ChatGPT Codex (OAuth)",
  "environment": {
    "temperature_actual": 0.1,
    "theo_version": "0.1.0"
  }
}
```

## `error_class` — the headline addition (v3)

Typed enum classifying WHY the run ended the way it did. Optional field
(omitted on legacy v2 paths). Snake_case serialization.

| Value | Meaning | `success` invariant |
|---|---|---|
| `solved` | Agent completed the task and `done` gate accepted | always `true` |
| `exhausted` | Iteration / token / budget limit hit before `done` | always `false` |
| `rate_limited` | Provider 429 throttling (TPM/RPM); retry exhausted | always `false` |
| `quota_exceeded` | Provider account hit hard usage cap (billing) | always `false` |
| `auth_failed` | Provider 401/403 — credentials missing or invalid | always `false` |
| `context_overflow` | Prompt exceeds the model's context window | always `false` |
| `sandbox_denied` | Tool blocked by bwrap/landlock/noop cascade | always `false` |
| `cancelled` | User Ctrl+C or parent agent abort (cooperative) | always `false` |
| `aborted` | Internal invariant broken (catch-all) | always `false` |
| `invalid_task` | Task description couldn't be parsed/validated | always `false` |

### Invariant

```
success == true  ⇔  error_class == "solved"
```

Validated by `cargo test -p theo-agent-runtime invariant_solved_iff_success_true`.

### Infra failures vs agent failures

For statistical comparison (e.g., A/B prompt testing), tools should
EXCLUDE infra failures from paired analysis — they reflect provider
state, not agent behavior:

```python
INFRA_FAILURE_CLASSES = {
    "rate_limited",       # transient throttling
    "quota_exceeded",     # billing cycle exhausted (no retry helps)
    "auth_failed",        # credentials invalid (no retry helps)
    "context_overflow",   # task structure problem
    "sandbox_denied",     # environment problem
}
```

`apps/theo-benchmark/runner/ab_compare.py` implements this exclusion via
`is_real_outcome()`.

## Backward compatibility

- v1 records (`schema: "theo.headless.v1"`): still accepted by
  `parse_result()` (loose prefix match). No `error_class`.
- v2 records (`schema: "theo.headless.v2"`): same shape as v3 minus
  `error_class`. Parsers default it to `null`/`None`.
- v3 records (`schema: "theo.headless.v3"`): includes `error_class`
  when known; field is omitted (NOT explicitly null) when the run
  predates classification migration.

Consumers that only care about `success` keep working unchanged.
Consumers that want richer classification opt in by reading
`data.get("error_class")`.

## Producer location

`apps/theo-cli/src/main.rs::cmd_headless` — the JSON literal at the end
of the headless code path, gated by `!headless` flag. Schema bump to v3
in commit (Phase 60).

## Pre-flight error format (separate)

When `theo --headless` fails BEFORE the agent loop (e.g., config error,
project dir invalid), it emits a minimal v1 record:

```json
{"schema": "theo.headless.v1", "success": false, "error": "<reason>"}
```

This path doesn't carry `error_class` because the failure happened
before the runtime initialized.
