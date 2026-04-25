#!/usr/bin/env bash
#
# REMEDIATION_PLAN T0.3 — Coverage gate for `theo-agent-runtime`.
#
# Re-runs `cargo tarpaulin`, parses the per-module line-rates from the
# resulting cobertura XML, and compares them against the canonical
# baseline TSV (most recent `.coverage/baseline-<sha>.tsv`). Fails the
# build if any tracked module's line-rate drops by more than
# `MAX_DROP_PP` percentage points.
#
# Usage:   scripts/check-coverage.sh
# Update:  re-run + commit `.coverage/baseline-<new-sha>.tsv`.
#
set -euo pipefail

MAX_DROP_PP="${MAX_DROP_PP:-2.0}"
COVERAGE_DIR=".coverage"
COBERTURA="${COVERAGE_DIR}/cobertura.xml"

# ── Locate the canonical baseline (lexicographically last is fine —
# git short SHAs sort by recency only by chance, so prefer mtime). ──
BASELINE="$(ls -t "${COVERAGE_DIR}"/baseline-*.tsv 2>/dev/null | head -n1 || true)"
if [[ -z "${BASELINE}" ]]; then
    echo "ERROR: no baseline TSV found in ${COVERAGE_DIR}/" >&2
    exit 1
fi
echo "Using baseline: ${BASELINE}"

# ── Run tarpaulin ──────────────────────────────────────────────────
mkdir -p "${COVERAGE_DIR}"
echo "Running cargo tarpaulin -p theo-agent-runtime ..."
cargo tarpaulin -p theo-agent-runtime \
    --out Xml \
    --output-dir "${COVERAGE_DIR}" \
    --skip-clean \
    --timeout 300 \
    --no-fail-fast \
    >/dev/null 2>&1 || true  # tarpaulin exits non-zero when any test
                              # fails; we still want the partial coverage
                              # data for the modules whose tests passed.

if [[ ! -f "${COBERTURA}" ]]; then
    echo "ERROR: tarpaulin did not produce ${COBERTURA}" >&2
    exit 1
fi

# ── Extract per-module line-rates into a TSV ───────────────────────
CURRENT_TSV="${COVERAGE_DIR}/current.tsv"
tr '<' '\n<' < "${COBERTURA}" \
    | grep 'package name="crates/theo-agent-runtime' \
    | sed 's/.*name="\(crates\/theo-agent-runtime[^"]*\)" line-rate="\([^"]*\)".*/\1\t\2/' \
    > "${CURRENT_TSV}"

# ── Compare ────────────────────────────────────────────────────────
declare -A baseline_rates
while IFS=$'\t' read -r module rate; do
    baseline_rates["${module}"]="${rate}"
done < "${BASELINE}"

regressions=0
echo
printf "%-60s %8s %8s %8s\n" "MODULE" "BASELINE" "CURRENT" "DROP_PP"
echo "--------------------------------------------------------------------------------------"
while IFS=$'\t' read -r module current; do
    baseline="${baseline_rates[${module}]:-}"
    if [[ -z "${baseline}" ]]; then
        printf "%-60s %8s %8s %8s   (new module — accepted)\n" \
            "${module}" "—" "${current}" "—"
        continue
    fi
    drop_pp="$(awk -v b="${baseline}" -v c="${current}" \
        'BEGIN { printf "%.2f", (b - c) * 100 }')"
    status=""
    if awk -v d="${drop_pp}" -v t="${MAX_DROP_PP}" \
        'BEGIN { exit !(d > t) }'; then
        status="  <-- REGRESSION"
        regressions=$((regressions + 1))
    fi
    printf "%-60s %8.4f %8.4f %8s%s\n" \
        "${module}" "${baseline}" "${current}" "${drop_pp}" "${status}"
done < "${CURRENT_TSV}"

echo
if [[ "${regressions}" -gt 0 ]]; then
    echo "FAIL: ${regressions} module(s) regressed by more than ${MAX_DROP_PP}pp." >&2
    echo "      Either fix the regression OR update the baseline:" >&2
    echo "        cp ${CURRENT_TSV} .coverage/baseline-\$(git rev-parse --short HEAD).tsv" >&2
    exit 1
fi
echo "OK: no module regressed by more than ${MAX_DROP_PP}pp."
