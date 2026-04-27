#!/usr/bin/env bash
# check-workspace-deps.sh
#
# Sixth surface for the CONTENT/STRUCTURAL pattern (iter-25..29):
# **cargo workspace dependencies**.
#
# CONTENT: every dep declared in `[workspace.dependencies]` of the
#   root `Cargo.toml`.
# STRUCTURAL: every workspace dep is referenced by at least one
#   crate via `<dep>.workspace = true` (in any of `[dependencies]`,
#   `[dev-dependencies]`, or `[build-dependencies]`).
#
# A workspace dep nobody uses is dead weight: it ends up in
# `Cargo.lock` (slowing fresh checkouts), might pull in a CVE
# nobody owns, and gets caught by `cargo-deny` only at the
# transitive level. This script catches them at the source.
#
# Same lesson as iter-25..29: a CONTENT audit (dep declared?)
# doesn't substitute for a STRUCTURAL audit (dep actually used?).
#
# Usage:
#   scripts/check-workspace-deps.sh           # strict
#   scripts/check-workspace-deps.sh --json    # CI consumption
#
# Exit codes:
#   0  every workspace dep is referenced by ≥1 crate
#   1  one or more deps are dead weight
#   2  invocation error

set -uo pipefail

OUTPUT="text"
for arg in "$@"; do
    case "$arg" in
        --json)    OUTPUT="json" ;;
        --help|-h) sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Extract every dep from the root [workspace.dependencies] table.
# ---------------------------------------------------------------------------

WORKSPACE_DEPS=$(awk '
    /^\[workspace\.dependencies\]/ { in_block = 1; next }
    /^\[/                          { in_block = 0 }
    in_block && /^[a-zA-Z]/ {
        sub(/^[[:space:]]*/, "")
        match($0, /^([a-zA-Z0-9_-]+)/, m)
        if (m[1] != "") print m[1]
    }
' Cargo.toml | sort -u)

# ---------------------------------------------------------------------------
# Walk every per-crate Cargo.toml under crates/ and apps/, collect
# the names of deps it declares with `.workspace = true`.
# ---------------------------------------------------------------------------

USED_DEPS=$(
    while IFS= read -r toml; do
        # `<name>.workspace = true` syntax: collapse name into $1.
        # Also catch `<name> = { workspace = true ... }` syntax.
        awk '
            /\.workspace[[:space:]]*=[[:space:]]*true/ {
                line = $0
                sub(/\.workspace.*/, "", line)
                sub(/^[[:space:]]*/, "", line)
                if (line != "") print line
            }
            /=[[:space:]]*\{[^}]*workspace[[:space:]]*=[[:space:]]*true/ {
                line = $0
                sub(/[[:space:]]*=.*/, "", line)
                sub(/^[[:space:]]*/, "", line)
                if (line != "") print line
            }
        ' "$toml"
    done < <(find crates apps -maxdepth 3 -name Cargo.toml -not -path "*/target/*") \
    | sort -u
)

# ---------------------------------------------------------------------------
# Compute the set difference: declared but not used.
# ---------------------------------------------------------------------------

UNUSED=$(comm -23 <(printf '%s\n' "$WORKSPACE_DEPS") <(printf '%s\n' "$USED_DEPS"))

total=$(printf '%s\n' "$WORKSPACE_DEPS" | grep -c .)
unused_count=$(printf '%s\n' "$UNUSED" | grep -c .)

# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------

if [[ "$OUTPUT" == "json" ]]; then
    printf '{\n  "total_workspace_deps": %d,\n' "$total"
    printf '  "unused": [\n'
    if [[ -n "$UNUSED" ]]; then
        first=1
        while IFS= read -r dep; do
            [[ -z "$dep" ]] && continue
            [[ $first -eq 0 ]] && printf ',\n'
            first=0
            printf '    "%s"' "$dep"
        done <<< "$UNUSED"
        printf '\n'
    fi
    printf '  ],\n  "unused_count": %d\n}\n' "$unused_count"
else
    printf 'Workspace deps coverage (declared → used)\n'
    printf '%s\n' "------------------------------------------------------------"
    printf '  declared in [workspace.dependencies]: %d\n' "$total"
    printf '  unused (no `<name>.workspace = true`): %d\n' "$unused_count"
    if [[ $unused_count -gt 0 ]]; then
        printf '\n  Declared-but-unused workspace deps:\n'
        while IFS= read -r dep; do
            [[ -z "$dep" ]] && continue
            printf '    [DEAD] %s\n' "$dep"
        done <<< "$UNUSED"
    fi
    printf '%s\n' "------------------------------------------------------------"
    if [[ $unused_count -gt 0 ]]; then
        printf '✗ %d workspace dep(s) are declared but unused.\n' "$unused_count"
        printf '  Either:\n'
        printf '    - Add `<name>.workspace = true` to a crate that\n'
        printf '      legitimately needs the dep, OR\n'
        printf '    - Remove the entry from root [workspace.dependencies]\n'
        printf '      so the version pin is not silently shipped to all\n'
        printf '      consumers via Cargo.lock.\n'
    else
        printf '✓ Every workspace dep is referenced by ≥1 crate.\n'
        printf '  No dead weight in [workspace.dependencies].\n'
    fi
fi

exit $(( unused_count > 0 ? 1 : 0 ))
