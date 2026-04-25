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
# Usage: scripts/check-module-size.sh
#
set -euo pipefail

#
# `MAX_LOC` is the regression-prevention cap. The long-term target
# from REMEDIATION_PLAN T4.* is 500, but several files still sit
# above (post-split mature modules with extensive docs). The cap is
# intentionally loose to catch *new* regressions without forcing a
# rewrite of every borderline file in one PR. Tighten when the
# remaining 5 modules clear the 500 line.
MAX_LOC="${MAX_LOC:-750}"
ROOT="${ROOT:-crates/theo-agent-runtime/src}"

if [[ ! -d "${ROOT}" ]]; then
    echo "ERROR: ${ROOT} not found" >&2
    exit 1
fi

regressions=0
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

    if (( prod_loc > MAX_LOC )); then
        printf '  REGRESSION: %s — %s production LOC > %s\n' \
            "${file}" "${prod_loc}" "${MAX_LOC}"
        regressions=$((regressions + 1))
    fi
done < <(find "${ROOT}" -name '*.rs' -type f -print0)

echo
echo "Scanned ${total_files} files in ${ROOT} (production LOC cap: ${MAX_LOC})."
if (( regressions > 0 )); then
    echo "FAIL: ${regressions} file(s) exceed ${MAX_LOC} production LOC." >&2
    echo "      Either split the file OR raise MAX_LOC with justification." >&2
    exit 1
fi
echo "OK: every module is at or below the ${MAX_LOC} production-LOC cap."
