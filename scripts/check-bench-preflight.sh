#!/usr/bin/env bash
# check-bench-preflight.sh
#
# Pre-flight validation of the SOTA benchmark infrastructure
# (`apps/theo-benchmark/` + `.github/workflows/eval.yml`) WITHOUT
# actually calling any LLM. The full SWE-Bench / terminal-bench
# eval (Global DoD #10/#11) needs paid LLM API and is genuinely
# OUT-OF-SCOPE for the autonomous loop, but everything UP TO the
# LLM call is gate-able locally:
#
#   1. The eval.yml workflow YAML parses.
#   2. The theo binary builds in release mode (the smoke job
#      shells out to `target/release/theo`).
#   3. `runner/smoke.py --help` works (Python imports + argparse
#      surface intact).
#   4. The smoke scenarios directory has parseable TOML files.
#   5. `analysis/report_builder.py` imports (consumed by smoke.py
#      via `from analysis.report_builder import build_report`).
#
# Effect: when a maintainer plugs `THEO_GROQ_API_KEY` into repo
# secrets, the `eval.yml::smoke` job runs immediately with zero
# scaffold surprises — every non-LLM step has been validated
# locally by this gate.
#
# Usage:
#   scripts/check-bench-preflight.sh           # strict
#   scripts/check-bench-preflight.sh --no-build  # skip cargo build
#   scripts/check-bench-preflight.sh --json    # CI consumption
#
# Exit codes:
#   0  every pre-flight check passes
#   1  one or more checks fail (the failure mode names what
#      a maintainer must fix before plugging in API keys)
#   2  invocation error

set -uo pipefail

OUTPUT="text"
NO_BUILD=0
for arg in "$@"; do
    case "$arg" in
        --json)     OUTPUT="json" ;;
        --no-build) NO_BUILD=1 ;;
        --help|-h)  sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

declare -a RESULTS=()

record() {
    # record <status> <label> [<detail>]
    local status="$1" label="$2" detail="${3:-}"
    RESULTS+=( "${status}|${label}|${detail}" )
}

# ---------------------------------------------------------------------------
# 1. eval.yml YAML parses
# ---------------------------------------------------------------------------

if [[ ! -f .github/workflows/eval.yml ]]; then
    record FAIL "eval.yml exists" "missing"
elif ! command -v python3 >/dev/null 2>&1; then
    record SKIP "eval.yml YAML parses" "python3 missing"
elif python3 -c "import yaml; yaml.safe_load(open('.github/workflows/eval.yml'))" 2>/dev/null; then
    record PASS "eval.yml YAML parses"
else
    record FAIL "eval.yml YAML parses" "python3 -c yaml.safe_load failed"
fi

# ---------------------------------------------------------------------------
# 2. theo binary builds (release)
# ---------------------------------------------------------------------------

if [[ $NO_BUILD -eq 1 ]]; then
    if [[ -x target/release/theo ]]; then
        record PASS "theo release binary present (--no-build)"
    else
        record SKIP "theo release binary build (--no-build)" "no existing binary"
    fi
else
    if cargo build --release --bin theo --quiet 2>/dev/null; then
        record PASS "cargo build --release --bin theo"
    else
        record FAIL "cargo build --release --bin theo"
    fi
fi

# ---------------------------------------------------------------------------
# 3. Every runner script under apps/theo-benchmark/runner/ has a
#    working `--help`. Validates the full bench scaffold (smoke +
#    A/B testing + monitoring + evolution + telemetry export) —
#    each runner is a piece of the SOTA bench surface that future
#    eval.yml jobs may invoke.
# ---------------------------------------------------------------------------

RUNNER_DIR="apps/theo-benchmark/runner"
if [[ ! -d "$RUNNER_DIR" ]]; then
    record FAIL "runner dir exists" "$RUNNER_DIR missing"
else
    runner_total=0
    runner_bad=0
    for f in "$RUNNER_DIR"/*.py; do
        [[ -e "$f" ]] || continue
        runner_total=$((runner_total + 1))
        if python3 "$f" --help >/dev/null 2>&1; then
            :
        else
            runner_bad=$((runner_bad + 1))
            record FAIL "runner --help" "$f"
        fi
    done
    if [[ $runner_total -eq 0 ]]; then
        record FAIL "runner scripts present" "$RUNNER_DIR has no .py runners"
    elif [[ $runner_bad -eq 0 ]]; then
        record PASS "all $runner_total runner --help (argparse + imports)"
    fi
fi

# ---------------------------------------------------------------------------
# 4. smoke scenarios parse as TOML
# ---------------------------------------------------------------------------

SCENARIOS_DIR="apps/theo-benchmark/scenarios/smoke"
if [[ ! -d "$SCENARIOS_DIR" ]]; then
    record FAIL "smoke scenarios dir exists" "$SCENARIOS_DIR missing"
else
    bad=0
    total=0
    for f in "$SCENARIOS_DIR"/*.toml; do
        [[ -e "$f" ]] || continue
        total=$((total + 1))
        if ! python3 -c "
import sys
try:
    import tomllib  # py 3.11+
except ImportError:
    import tomli as tomllib  # py 3.10-
tomllib.load(open(sys.argv[1], 'rb'))
" "$f" 2>/dev/null; then
            bad=$((bad + 1))
            record FAIL "scenario parses" "$f"
        fi
    done
    if [[ $bad -eq 0 && $total -gt 0 ]]; then
        record PASS "all $total smoke scenarios parse as TOML"
    elif [[ $total -eq 0 ]]; then
        record FAIL "smoke scenarios non-empty" "$SCENARIOS_DIR has no .toml files"
    fi
fi

# ---------------------------------------------------------------------------
# 5. Every analysis module under apps/theo-benchmark/analysis/
#    imports cleanly. The SOTA report (Phase 64) chain pulls in
#    report_builder + a few siblings; if any module has a syntax
#    error or missing dep, smoke.py falls back to a degraded
#    report at runtime — pre-flight catches it earlier.
# ---------------------------------------------------------------------------

ANALYSIS_DIR="apps/theo-benchmark/analysis"
if [[ ! -d "$ANALYSIS_DIR" ]]; then
    record FAIL "analysis dir exists" "$ANALYSIS_DIR missing"
else
    analysis_total=0
    analysis_bad=0
    while IFS= read -r f; do
        analysis_total=$((analysis_total + 1))
        modname="$(basename "$f" .py)"
        # Run the import from the bench root so `analysis.` is on the path.
        if ( cd apps/theo-benchmark && python3 -c "from analysis import $modname" 2>/dev/null ); then
            :
        else
            analysis_bad=$((analysis_bad + 1))
            record FAIL "analysis module imports" "$f"
        fi
    done < <(find "$ANALYSIS_DIR" -maxdepth 1 -name "*.py" ! -name "__init__.py" | sort)
    if [[ $analysis_total -eq 0 ]]; then
        record FAIL "analysis modules present" "$ANALYSIS_DIR has no .py modules"
    elif [[ $analysis_bad -eq 0 ]]; then
        record PASS "all $analysis_total analysis modules import"
    fi
    # Extra: confirm the specific symbols smoke.py uses still resolve.
    if ( cd apps/theo-benchmark && python3 -c "from analysis.report_builder import build_report, report_to_markdown" 2>/dev/null ); then
        record PASS "analysis.report_builder.{build_report,report_to_markdown} resolved"
    else
        record FAIL "analysis.report_builder symbol resolution"
    fi
fi

# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------

failed=0

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n  "checks": [\n'
    first=1
    for r in "${RESULTS[@]}"; do
        IFS='|' read -r status label detail <<< "$r"
        [[ $first -eq 0 ]] && printf ',\n'
        first=0
        printf '    {"status": "%s", "label": "%s", "detail": "%s"}' \
            "$status" "$label" "$detail"
        [[ "$status" == "FAIL" ]] && failed=$((failed + 1))
    done
    printf '\n  ],\n  "failed": %d\n}\n' "$failed"
else
    printf 'SOTA bench infrastructure pre-flight\n'
    printf '%s\n' "------------------------------------------------------------"
    for r in "${RESULTS[@]}"; do
        IFS='|' read -r status label detail <<< "$r"
        case "$status" in
            PASS) printf '  [PASS] %s\n' "$label" ;;
            SKIP) printf '  [SKIP] %s — %s\n' "$label" "$detail" ;;
            FAIL)
                if [[ -n "$detail" ]]; then
                    printf '  [FAIL] %s — %s\n' "$label" "$detail"
                else
                    printf '  [FAIL] %s\n' "$label"
                fi
                failed=$((failed + 1))
                ;;
        esac
    done
    printf '%s\n' "------------------------------------------------------------"
    if [[ $failed -gt 0 ]]; then
        printf '✗ %d pre-flight check(s) failed.\n' "$failed"
        printf '  Fix above before plugging in THEO_GROQ_API_KEY /\n'
        printf '  ANTHROPIC_API_KEY / OPENAI_API_KEY into repo secrets.\n'
    else
        printf '✓ Bench infrastructure ready. When a maintainer plugs in\n'
        printf '  THEO_GROQ_API_KEY (smoke) or ANTHROPIC_API_KEY /\n'
        printf '  OPENAI_API_KEY (full bench), eval.yml runs immediately.\n'
        printf '  Global DoD #10/#11 unblocked from the infrastructure side.\n'
    fi
fi

exit $(( failed > 0 ? 1 : 0 ))
