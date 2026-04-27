#!/usr/bin/env bash
# check-coverage-status.sh
#
# Closes the `coverage` half of Global DoD #6 of
# `docs/plans/sota-tier1-tier2-plan.md` for LOCAL workflows.
#
# `cargo tarpaulin` is heavy (minutes per run) so the DoD-report
# runner can't afford to re-run it every iteration. The full
# tarpaulin gate is wired into CI via `audit.yml::coverage`
# (which calls `scripts/check-coverage.sh`); this script is the
# fast LOCAL companion that:
#
#   1. Validates the LAST locally-produced `.coverage/cobertura.xml`
#      exists and is parseable.
#   2. Extracts the workspace top-level `line-rate` and reports it.
#   3. Reports the age of the artifact so stale runs are visible.
#   4. Exits 0 when a valid artifact exists with line-rate >= MIN_RATE
#      (default 0.30), 1 when missing/broken/below floor.
#
# To refresh the local artifact:
#   cargo tarpaulin -p theo-agent-runtime --out Xml --output-dir .coverage
#
# Usage:
#   scripts/check-coverage-status.sh           # strict
#   scripts/check-coverage-status.sh --report  # never fail
#
# Exit codes:
#   0  cobertura.xml exists, parses, and rate >= MIN_RATE
#   1  missing / unparseable / rate below floor
#   2  invocation error

set -euo pipefail

MIN_RATE="${MIN_RATE:-0.30}"
MODE="strict"
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --help|-h) sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

COBERTURA=".coverage/cobertura.xml"

# ---------------------------------------------------------------------------
# Validate artifact
# ---------------------------------------------------------------------------

if [[ ! -f "$COBERTURA" ]]; then
    printf '✗ %s missing.\n' "$COBERTURA" >&2
    printf '  Run: cargo tarpaulin -p theo-agent-runtime --out Xml \\\n' >&2
    printf '         --output-dir .coverage\n' >&2
    printf '  CI runs the full gate via audit.yml::coverage.\n' >&2
    [[ "$MODE" == "strict" ]] && exit 1 || exit 0
fi

# Workspace-level line-rate is on the FIRST <coverage line-rate="..."> tag.
# Extract just the first occurrence via awk to avoid SIGPIPE from
# `grep | head -1` under `set -o pipefail`.
RATE="$(awk '
    match($0, /line-rate="[0-9.]+"/) {
        s = substr($0, RSTART + 11, RLENGTH - 12)
        print s
        exit
    }
' "$COBERTURA")"

if [[ -z "$RATE" ]]; then
    printf '✗ %s has no parseable line-rate.\n' "$COBERTURA" >&2
    [[ "$MODE" == "strict" ]] && exit 1 || exit 0
fi

# ---------------------------------------------------------------------------
# Age check (informational)
# ---------------------------------------------------------------------------

# `stat -c %Y` is GNU; fall back to BSD/mac.
if mtime=$(stat -c %Y "$COBERTURA" 2>/dev/null); then
    :
elif mtime=$(stat -f %m "$COBERTURA" 2>/dev/null); then
    :
else
    mtime=$(date +%s)
fi
now=$(date +%s)
age_days=$(( (now - mtime) / 86400 ))

# ---------------------------------------------------------------------------
# Compare to floor
# ---------------------------------------------------------------------------

# Use awk for float comparison (bash has no native).
ok=$(awk -v r="$RATE" -v m="$MIN_RATE" 'BEGIN { print (r >= m) ? 1 : 0 }')

printf 'coverage gate (local artifact validation)\n'
printf '%s\n' "------------------------------------------------------------"
printf '  artifact:  %s\n' "$COBERTURA"
printf '  age:       %d day(s)\n' "$age_days"
printf '  line-rate: %s  (floor: %s)\n' "$RATE" "$MIN_RATE"
printf '%s\n' "------------------------------------------------------------"

if [[ "$ok" -ne 1 ]]; then
    printf '✗ line-rate %s is below the %s floor.\n' "$RATE" "$MIN_RATE" >&2
    [[ "$MODE" == "strict" ]] && exit 1 || exit 0
fi

if [[ "$age_days" -gt 30 ]]; then
    printf '⚠ artifact is %d days old. Refresh:\n' "$age_days"
    printf '  cargo tarpaulin -p theo-agent-runtime --out Xml \\\n'
    printf '    --output-dir .coverage\n'
    # Don't fail on age — informational only.
fi

printf '✓ coverage artifact valid (line-rate >= floor).\n'
printf '  Full per-module gate runs in CI: audit.yml::coverage.\n'
exit 0
