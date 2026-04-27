#!/usr/bin/env bash
# Regression test for the SOTA Global DoD gate scripts.
#
# Tests the 4 gate scripts I shipped in iterations 17-19 plus the
# DoD aggregator runner:
#
#   - scripts/check-adr-coverage.sh
#   - scripts/check-complexity.sh
#   - scripts/check-coverage-status.sh
#   - scripts/check-changelog-phase-coverage.sh
#   - scripts/check-sota-dod.sh
#
# Each gate is exercised in three modes:
#
#   1. Default mode against the current repo state (must pass).
#   2. `--help` (must exit 0; usage block must be non-empty).
#   3. Bogus argument (must exit 2 = invocation error).
#
# Plus per-gate semantic tests:
#
#   - ADR coverage `--json` produces parseable JSON.
#   - CHANGELOG phase coverage `--json` produces parseable JSON.
#   - Coverage status with `MIN_RATE=99.0` fails (artificially
#     high floor).
#   - Complexity gate `--report` never fails (report mode only).
#
# Modeled on `scripts/check-arch-contract.test.sh` — same pattern,
# same exit semantics.
#
# Usage:   bash scripts/check-sota-dod.test.sh
# Exit 0 = all assertions pass; exit 1 = at least one failure.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

pass=0
fail=0

# ---------------------------------------------------------------------------
# Assertion helpers
# ---------------------------------------------------------------------------

assert_exits() {
    local label="$1" want="$2"
    shift 2
    local got
    "$@" >/dev/null 2>&1
    got=$?
    if [[ "$got" -eq "$want" ]]; then
        printf '  pass: %s (exit %d)\n' "$label" "$got"
        pass=$((pass + 1))
    else
        printf '  FAIL: %s — wanted exit %d, got %d (%s)\n' \
            "$label" "$want" "$got" "$*" >&2
        fail=$((fail + 1))
    fi
}

assert_help_nonempty() {
    local label="$1" script="$2"
    local out
    out="$(bash "$script" --help 2>&1 || true)"
    if [[ -n "$out" ]]; then
        printf '  pass: %s --help yields non-empty output (%d bytes)\n' "$label" "${#out}"
        pass=$((pass + 1))
    else
        printf '  FAIL: %s --help is empty\n' "$label" >&2
        fail=$((fail + 1))
    fi
}

assert_json_parseable() {
    local label="$1"
    shift
    local out
    out="$("$@" 2>/dev/null || true)"
    if printf '%s' "$out" | python3 -c 'import sys, json; json.load(sys.stdin)' 2>/dev/null; then
        printf '  pass: %s emits valid JSON\n' "$label"
        pass=$((pass + 1))
    else
        printf '  FAIL: %s did not emit valid JSON\n' "$label" >&2
        fail=$((fail + 1))
    fi
}

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------

# Confirm every script exists and is executable.
for s in check-adr-coverage.sh check-complexity.sh \
         check-coverage-status.sh check-changelog-phase-coverage.sh \
         check-sota-dod.sh; do
    if [[ ! -x "scripts/$s" ]]; then
        echo "FATAL: scripts/$s missing or not executable" >&2
        exit 1
    fi
done

printf 'SOTA-DoD gate regression tests\n'
printf '%s\n' "------------------------------------------------------------"

# ---------------------------------------------------------------------------
# 1. Default mode against current repo (must pass)
# ---------------------------------------------------------------------------

printf '\n[1] Default-mode pass on current repo state\n'
assert_exits "check-adr-coverage.sh"               0 \
    bash scripts/check-adr-coverage.sh
assert_exits "check-complexity.sh"                 0 \
    bash scripts/check-complexity.sh
assert_exits "check-coverage-status.sh"            0 \
    bash scripts/check-coverage-status.sh
assert_exits "check-changelog-phase-coverage.sh"   0 \
    bash scripts/check-changelog-phase-coverage.sh

# ---------------------------------------------------------------------------
# 2. --help mode (exit 0, non-empty output)
# ---------------------------------------------------------------------------

printf '\n[2] --help mode yields non-empty usage block\n'
assert_help_nonempty "check-adr-coverage.sh"             scripts/check-adr-coverage.sh
assert_help_nonempty "check-complexity.sh"               scripts/check-complexity.sh
assert_help_nonempty "check-coverage-status.sh"          scripts/check-coverage-status.sh
assert_help_nonempty "check-changelog-phase-coverage.sh" scripts/check-changelog-phase-coverage.sh

# ---------------------------------------------------------------------------
# 3. Bogus argument (exit 2 = invocation error)
# ---------------------------------------------------------------------------

printf '\n[3] Bogus argument yields exit 2\n'
assert_exits "check-adr-coverage.sh --bogus"             2 \
    bash scripts/check-adr-coverage.sh --bogus
assert_exits "check-complexity.sh --bogus"               2 \
    bash scripts/check-complexity.sh --bogus
assert_exits "check-coverage-status.sh --bogus"          2 \
    bash scripts/check-coverage-status.sh --bogus
assert_exits "check-changelog-phase-coverage.sh --bogus" 2 \
    bash scripts/check-changelog-phase-coverage.sh --bogus

# ---------------------------------------------------------------------------
# 4. Per-gate semantic tests
# ---------------------------------------------------------------------------

printf '\n[4] Per-gate semantic tests\n'

# ADR coverage --json must be parseable JSON.
assert_json_parseable "ADR coverage --json" \
    bash scripts/check-adr-coverage.sh --json

# CHANGELOG phase coverage --json must be parseable JSON.
assert_json_parseable "CHANGELOG phase coverage --json" \
    bash scripts/check-changelog-phase-coverage.sh --json

# Complexity gate --report mode never fails (always exit 0).
assert_exits "complexity --report never fails"           0 \
    bash scripts/check-complexity.sh --report

# Coverage status with artificially high MIN_RATE must FAIL.
# (today the line-rate is ~38.56%; floor 99.0 is unreachable.)
MIN_RATE=99.0 assert_exits "coverage MIN_RATE=99.0 (must fail)" 1 \
    env MIN_RATE=99.0 bash scripts/check-coverage-status.sh

# Coverage status with artificially low MIN_RATE must PASS.
MIN_RATE=0.01 assert_exits "coverage MIN_RATE=0.01 (must pass)" 0 \
    env MIN_RATE=0.01 bash scripts/check-coverage-status.sh

# ---------------------------------------------------------------------------
# 5. DoD runner: --quick exits 0 (all gate-able items PASS today)
# ---------------------------------------------------------------------------

printf '\n[5] check-sota-dod.sh aggregator (--quick)\n'
assert_exits "check-sota-dod.sh --quick" 0 \
    bash scripts/check-sota-dod.sh --quick
assert_help_nonempty "check-sota-dod.sh" scripts/check-sota-dod.sh

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

printf '\n%s\n' "------------------------------------------------------------"
printf '%d pass, %d fail\n' "$pass" "$fail"
if [[ "$fail" -gt 0 ]]; then
    printf '✗ at least one assertion failed.\n' >&2
    exit 1
fi
printf '✓ every assertion holds.\n'
exit 0
