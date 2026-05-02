#!/usr/bin/env bash
#
# REMEDIATION_PLAN T8.5 — Module-size CI gate for `theo-agent-runtime`.
#
# Counts PRODUCTION-only LOC in each .rs file under
# `crates/theo-agent-runtime/src/` (excludes the `#[cfg(test)] mod tests`
# block by stopping the count at the first such marker). Fails the
# build when any file exceeds `MAX_LOC` (default 500).
#
# Why production-only: god-files have already been split, but mature
# files still carry large `#[cfg(test)] mod tests` blocks (sometimes
# 500-700 LOC of unit tests). Counting tests would make the gate
# noisy without reflecting structural debt.
#
# Allowlist: shares `.claude/rules/size-allowlist.txt` with
# `scripts/check-sizes.sh`. Each line `<path>|<limit>|<sunset>|<reason>`
# bumps the cap for one specific file until the sunset date. After
# the sunset the gate fails again unless the file has been brought
# back under the default `MAX_LOC` or the entry has been refreshed.
#
# Usage: scripts/check-module-size.sh
#
set -euo pipefail

#
# `MAX_LOC` is the regression-prevention cap, set to the long-term
# target from REMEDIATION_PLAN T4.* (500). All modules in
# `theo-agent-runtime/src/` cleared this threshold by Iter 61 — the
# cap exists to keep them there. Raising it is a regression and
# requires explicit justification in the PR description.
MAX_LOC="${MAX_LOC:-500}"
ROOT="${ROOT:-crates/theo-agent-runtime/src}"

if [[ ! -d "${ROOT}" ]]; then
    echo "ERROR: ${ROOT} not found" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Allowlist parsing — same format as scripts/check-sizes.sh so the two
# gates share a single source of truth.
# ---------------------------------------------------------------------------
ALLOWLIST_FILE=".claude/rules/size-allowlist.txt"
TODAY="$(date -u +%Y-%m-%d)"

declare -A ALLOW_LIMIT
declare -A ALLOW_SUNSET
if [[ -f "${ALLOWLIST_FILE}" ]]; then
    while IFS='|' read -r path limit sunset _reason; do
        [[ -z "${path// }" || "${path:0:1}" == "#" ]] && continue
        ALLOW_LIMIT["${path}"]="${limit}"
        ALLOW_SUNSET["${path}"]="${sunset}"
    done < "${ALLOWLIST_FILE}"
fi

# Returns the effective LOC ceiling for a path: allowlist value when
# present and not-yet-sunset; default `MAX_LOC` otherwise.
effective_cap_for() {
    local path="$1"
    local limit="${ALLOW_LIMIT[${path}]:-}"
    local sunset="${ALLOW_SUNSET[${path}]:-}"
    if [[ -n "${limit}" && -n "${sunset}" && "${sunset}" > "${TODAY}" ]]; then
        echo "${limit}"
        return
    fi
    echo "${MAX_LOC}"
}

regressions=0
allowed_overshoots=0
total_files=0

while IFS= read -r -d '' file; do
    total_files=$((total_files + 1))
    # awk: count non-blank, non-comment-only lines BEFORE the first
    # `#[cfg(test)]` line. Comment-only lines still count toward LOC
    # because doc comments are part of the API surface, but blank
    # lines are excluded so the threshold tracks signal density.
    prod_loc="$(awk '
        /^#\[cfg\(test\)\]/ { exit }
        /^[[:space:]]*$/    { next }
        { count++ }
        END { print count + 0 }
    ' "${file}")"

    cap="$(effective_cap_for "${file}")"

    if (( prod_loc > cap )); then
        # If the file is allowlisted, it means even the bumped ceiling
        # is exceeded — this is a real regression.
        if [[ -n "${ALLOW_LIMIT[${file}]:-}" ]]; then
            printf '  REGRESSION (allowlisted file over its bumped cap): %s — %s production LOC > %s (allowlisted ceiling)\n' \
                "${file}" "${prod_loc}" "${cap}"
        else
            printf '  REGRESSION: %s — %s production LOC > %s (default cap)\n' \
                "${file}" "${prod_loc}" "${cap}"
        fi
        regressions=$((regressions + 1))
    elif (( prod_loc > MAX_LOC )); then
        # Within allowlist ceiling but above the default cap — track
        # for visibility without failing.
        allowed_overshoots=$((allowed_overshoots + 1))
    fi
done < <(find "${ROOT}" -name '*.rs' -type f -print0)

echo
echo "Scanned ${total_files} files in ${ROOT} (default production LOC cap: ${MAX_LOC})."
if (( allowed_overshoots > 0 )); then
    echo "  ${allowed_overshoots} file(s) exceed default cap but are within their allowlist ceiling (size-allowlist.txt)."
fi
if (( regressions > 0 )); then
    echo "FAIL: ${regressions} file(s) exceed their effective production-LOC cap." >&2
    echo "      Either split the file OR add/refresh an entry in" >&2
    echo "      .claude/rules/size-allowlist.txt with explicit justification." >&2
    exit 1
fi
echo "OK: every module is at or below its effective production-LOC cap."
