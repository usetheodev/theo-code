#!/usr/bin/env bash
# check-arch-contract.sh
#
# T1.5 — Architectural boundary gate.
#
# Validates that every crate/app respects the dependency contract
# declared in `.claude/rules/architecture.md` /
# `.claude/rules/architecture-contract.yaml`:
#
#   theo-domain         → (nothing)
#   theo-engine-graph   → theo-domain
#   theo-engine-parser  → theo-domain
#   theo-engine-retrieval → theo-domain, theo-engine-graph, theo-engine-parser
#   theo-engine-wiki    → theo-domain, theo-engine-graph, theo-engine-parser
#   theo-governance     → theo-domain only
#   theo-isolation      → theo-domain only
#   theo-infra-llm      → theo-domain only
#   theo-infra-auth     → theo-domain only
#   theo-infra-mcp      → theo-domain only
#   theo-infra-memory   → theo-domain, theo-engine-retrieval
#   theo-tooling        → theo-domain only
#   theo-agent-runtime  → theo-domain, theo-governance,
#                         theo-infra-llm, theo-infra-auth,
#                         theo-tooling, theo-isolation, theo-infra-mcp
#   theo-api-contracts  → theo-domain only
#   theo-application    → all crates above
#   apps/*              → theo-application, theo-api-contracts, theo-domain
#
# The contract is embedded below as bash associative arrays so the
# script has zero runtime dependencies.
#
# Exit codes:
#   0  no violations
#   1  one or more violations detected
#   2  invocation error
#
# Usage:
#   scripts/check-arch-contract.sh              # strict (default, fail on first violation type summary)
#   scripts/check-arch-contract.sh --report     # report-only, exit 0
#   scripts/check-arch-contract.sh --json       # machine-readable output

set -euo pipefail

MODE="strict"
SOURCE_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --report) MODE="report" ;;
        --json)   MODE="json" ;;
        --source-only) SOURCE_ONLY=1 ;;
        --help|-h)
            sed -n '2,30p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

# --- Contract ---------------------------------------------------------------
# ALLOWED_DEPS[<cargo-path>] = "crate1 crate2 ..." (space-separated).
# Empty string means: no theo-* workspace deps allowed.
declare -A ALLOWED_DEPS=(
    ["crates/theo-domain"]=""
    ["crates/theo-engine-graph"]="theo-domain"
    ["crates/theo-engine-parser"]="theo-domain"
    ["crates/theo-engine-retrieval"]="theo-domain theo-engine-graph theo-engine-parser"
    ["crates/theo-engine-wiki"]="theo-domain theo-engine-graph theo-engine-parser"
    ["crates/theo-governance"]="theo-domain"
    ["crates/theo-isolation"]="theo-domain"
    ["crates/theo-infra-llm"]="theo-domain"
    ["crates/theo-infra-auth"]="theo-domain"
    ["crates/theo-infra-mcp"]="theo-domain"
    ["crates/theo-infra-memory"]="theo-domain theo-engine-retrieval"
    ["crates/theo-tooling"]="theo-domain"
    # ADR-021 (theo-isolation) + ADR-022 (theo-infra-mcp) authorize these deps.
    ["crates/theo-agent-runtime"]="theo-domain theo-governance theo-infra-llm theo-infra-auth theo-tooling theo-isolation theo-infra-mcp"
    ["crates/theo-api-contracts"]="theo-domain"
    # `theo-application` aggregates all runtime + engine crates; ADR-021/ADR-022
    # propagate transitively to it.
    ["crates/theo-application"]="theo-domain theo-engine-graph theo-engine-parser theo-engine-retrieval theo-engine-wiki theo-governance theo-infra-llm theo-infra-auth theo-infra-memory theo-tooling theo-agent-runtime theo-api-contracts theo-isolation theo-infra-mcp"
    ["crates/theo-test-memory-fixtures"]="theo-domain theo-infra-memory"
    # ADR-023 SUNSET (T3.3 done) — `apps/theo-cli` no longer imports
    # `theo-agent-runtime` directly; the `cli_runtime` re-export module
    # in `theo-application` is the contract surface.
    ["apps/theo-cli"]="theo-application theo-api-contracts theo-domain"
    ["apps/theo-desktop"]="theo-application theo-api-contracts theo-domain"
    ["apps/theo-marklive"]="theo-application theo-api-contracts theo-domain"
)

# Every crate name the contract recognises (used to detect imports of
# a forbidden crate vs unrelated code).
ALL_WORKSPACE_CRATES=(
    theo-domain theo-engine-graph theo-engine-parser theo-engine-retrieval
    theo-engine-wiki theo-governance theo-infra-llm theo-infra-auth theo-infra-memory
    theo-tooling theo-agent-runtime theo-api-contracts theo-application
    theo-test-memory-fixtures theo-isolation theo-infra-mcp
)

# --- Helpers ----------------------------------------------------------------

violations_cargo=()   # "crate/path: forbidden dep 'X' (allowed: Y Z)"
violations_import=()  # "crate/path/src/a.rs: forbidden import theo_x"

# Extract declared theo-* deps from a Cargo.toml [dependencies] / [dev-dependencies]?
# Rule: we count runtime deps only — skip lines inside [dev-dependencies], [build-dependencies], [target.'...].
declared_theo_deps() {
    local cargo="$1"
    local in_deps=0
    while IFS= read -r line; do
        # Strip comments and trim
        local stripped="${line%%#*}"
        stripped="${stripped##[[:space:]]}"
        # Section headers
        if [[ "$stripped" =~ ^\[[a-zA-Z0-9_.\'\"-]+\] ]]; then
            case "$stripped" in
                "[dependencies]") in_deps=1 ;;
                "[dependencies.theo-"*"]")
                    in_deps=1
                    # Print the bracketed crate directly
                    local name="${stripped#[dependencies.}"
                    name="${name%]}"
                    printf '%s\n' "$name"
                    ;;
                *) in_deps=0 ;;
            esac
            continue
        fi
        (( in_deps )) || continue
        # Match both inline form `theo-foo = ...` and workspace form
        # `theo-foo.workspace = true`. The optional `(\.workspace)?`
        # group covers the workspace syntax — without it, deps that
        # use `.workspace = true` slip past the gate (find_p5_001).
        if [[ "$stripped" =~ ^(theo-[a-zA-Z0-9_-]+)(\.workspace)?[[:space:]]*=.+ ]]; then
            printf '%s\n' "${BASH_REMATCH[1]}"
        fi
    done < "$cargo" | sort -u
}

# Convert "theo-agent-runtime" → "theo_agent_runtime"
to_crate_ident() { printf '%s' "${1//-/_}"; }

# Check whether a dep is allowed for a given crate (self-reference is trivially allowed).
dep_is_allowed() {
    local crate_path="$1" dep="$2" self_crate="$3"
    [[ "$dep" == "$self_crate" ]] && return 0
    local allowed="${ALLOWED_DEPS[$crate_path]-UNDEFINED}"
    if [[ "$allowed" == "UNDEFINED" ]]; then
        return 1   # crate not in contract ⇒ treat as strict (nothing allowed)
    fi
    for a in $allowed; do
        [[ "$a" == "$dep" ]] && return 0
    done
    return 1
}

# --- Scan -------------------------------------------------------------------

# `--source-only` exits here so callers can `source` the script and reuse
# the helper functions (`declared_theo_deps`, `dep_is_allowed`, etc.) for
# unit testing without triggering the full scan.
if (( SOURCE_ONLY )); then
    return 0 2>/dev/null || exit 0
fi

total_crates=0
ok_crates=0

for crate_path in "${!ALLOWED_DEPS[@]}"; do
    cargo="$crate_path/Cargo.toml"
    if [[ ! -f "$cargo" ]]; then
        echo "warn: $cargo missing, skipping" >&2
        continue
    fi
    total_crates=$((total_crates + 1))
    self_crate="$(basename "$crate_path")"
    # Cargo dep check
    while IFS= read -r dep; do
        [[ -z "$dep" ]] && continue
        if ! dep_is_allowed "$crate_path" "$dep" "$self_crate"; then
            violations_cargo+=("$crate_path: forbidden Cargo dep '$dep' (allowed: ${ALLOWED_DEPS[$crate_path]:-<none>})")
        fi
    done < <(declared_theo_deps "$cargo")

    # Source-level import check
    # Walk src/ but ignore tests/ subdirs that are inside src (some crates embed).
    if [[ -d "$crate_path/src" ]]; then
        # shellcheck disable=SC2044
        while IFS= read -r -d '' rs_file; do
            # Extract `use theo_<name>` / `use ::theo_<name>` prefixes on non-comment lines.
            while IFS= read -r match; do
                [[ -z "$match" ]] && continue
                local_ident="$match"
                # Reject if this imports a workspace crate not allowed.
                for wc in "${ALL_WORKSPACE_CRATES[@]}"; do
                    ident="$(to_crate_ident "$wc")"
                    if [[ "$local_ident" == "$ident" ]]; then
                        if ! dep_is_allowed "$crate_path" "$wc" "$self_crate"; then
                            violations_import+=("$rs_file: forbidden import '$ident' (allowed crates: ${ALLOWED_DEPS[$crate_path]:-<none>})")
                        fi
                    fi
                done
            done < <(
                grep -E '^[[:space:]]*(pub[[:space:]]+)?use[[:space:]]+(::)?theo_[a-z_]+' "$rs_file" 2>/dev/null \
                    | sed -E 's/^[[:space:]]*(pub[[:space:]]+)?use[[:space:]]+(::)?([a-z_][a-z0-9_]*).*/\3/' \
                    | sort -u
            )
        done < <(find "$crate_path/src" -type f -name '*.rs' -print0)
    fi
    ok_crates=$((ok_crates + 1))
done

# --- Report -----------------------------------------------------------------

total_violations=$(( ${#violations_cargo[@]} + ${#violations_import[@]} ))

if [[ "$MODE" == "json" ]]; then
    printf '{\n'
    printf '  "total_crates": %d,\n' "$total_crates"
    printf '  "total_violations": %d,\n' "$total_violations"
    printf '  "cargo_violations": ['
    first=1
    for v in "${violations_cargo[@]}"; do
        (( first )) || printf ','; first=0
        printf '\n    %s' "\"${v//\"/\\\"}\""
    done
    printf '\n  ],\n  "import_violations": ['
    first=1
    for v in "${violations_import[@]}"; do
        (( first )) || printf ','; first=0
        printf '\n    %s' "\"${v//\"/\\\"}\""
    done
    printf '\n  ]\n}\n'
else
    printf 'arch-contract check\n'
    printf '  crates scanned: %d\n' "$total_crates"
    printf '  total violations: %d\n\n' "$total_violations"
    if (( ${#violations_cargo[@]} > 0 )); then
        printf 'Cargo.toml dependency violations (%d):\n' "${#violations_cargo[@]}"
        for v in "${violations_cargo[@]}"; do printf '  - %s\n' "$v"; done
        printf '\n'
    fi
    if (( ${#violations_import[@]} > 0 )); then
        printf 'Source-level import violations (%d):\n' "${#violations_import[@]}"
        for v in "${violations_import[@]}"; do printf '  - %s\n' "$v"; done
        printf '\n'
    fi
    if (( total_violations == 0 )); then
        printf 'OK — contract respected.\n'
    fi
fi

if [[ "$MODE" == "strict" ]] && (( total_violations > 0 )); then
    exit 1
fi
exit 0
