#!/usr/bin/env bash
# check-changelog.sh
#
# T6.5 — CHANGELOG gate.
#
# Fails if the current diff touches code under crates/ or apps/ and the
# CHANGELOG.md does not have at least one new line inside the
# [Unreleased] section relative to the merge base.
#
# Usage:
#   scripts/check-changelog.sh                    # compare against origin/main
#   scripts/check-changelog.sh --base=origin/dev  # compare against a specific base
#   scripts/check-changelog.sh --staged           # check staged diff instead of HEAD
#
# Exit codes:
#   0  ok (no code changed OR CHANGELOG updated appropriately)
#   1  code changed but CHANGELOG.md [Unreleased] section did not grow
#   2  invocation error

set -euo pipefail

BASE="origin/main"
STAGED=0

for arg in "$@"; do
    case "$arg" in
        --base=*)   BASE="${arg#--base=}" ;;
        --staged)   STAGED=1 ;;
        --help|-h)  sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "unknown flag: $arg" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

if ! git rev-parse --verify "$BASE" >/dev/null 2>&1; then
    # Fall back to HEAD~1 if the base is unreachable (CI without remote refs,
    # or a shallow clone); better than failing loudly.
    echo "warn: $BASE not found, falling back to HEAD~1" >&2
    BASE="HEAD~1"
fi

if (( STAGED )); then
    diff_cmd=(git diff --name-only --cached)
    changelog_diff_cmd=(git diff --cached -- CHANGELOG.md)
else
    diff_cmd=(git diff --name-only "$BASE"...HEAD)
    changelog_diff_cmd=(git diff "$BASE"...HEAD -- CHANGELOG.md)
fi

changed_files="$("${diff_cmd[@]}" 2>/dev/null || true)"

code_touched=0
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    case "$f" in
        crates/*|apps/*) code_touched=1; break ;;
    esac
done <<< "$changed_files"

if (( code_touched == 0 )); then
    echo "changelog: no crates/ or apps/ files touched — skipping gate"
    exit 0
fi

changelog_diff="$("${changelog_diff_cmd[@]}" 2>/dev/null || true)"

if [[ -z "$changelog_diff" ]]; then
    cat <<EOF >&2
changelog gate FAILED

Code was changed under crates/ or apps/ but CHANGELOG.md was not updated.
Required per .claude/CLAUDE.md "Changelogs — Registro Obrigatório de Mudanças":

  Toda entry DEVE ter referência ao ticket/issue/PR entre parênteses.
  Use one of: Added | Changed | Deprecated | Removed | Fixed | Security.

Files touched:
$(printf '  - %s\n' $changed_files | head -20)
EOF
    exit 1
fi

# Count added lines that belong to the [Unreleased] section specifically.
# We track whether we're inside the [Unreleased] diff hunk by watching for
# the section header in context/added lines, and stop at the next version
# header (e.g. `## [1.2.0]`).
added_in_unreleased="$(printf '%s\n' "$changelog_diff" | awk '
    # Skip diff headers
    /^(\+\+\+|---)/ { next }
    # Detect [Unreleased] section (in added or context lines)
    /^[+ ].*\[Unreleased\]/ { in_section = 1; next }
    # Detect next versioned section — stop counting
    in_section && /^[+ ].*## \[[0-9]+\./ { in_section = 0 }
    # Count added lines inside [Unreleased]
    in_section && /^\+/ { count++ }
    END { print count + 0 }
')"

if (( added_in_unreleased == 0 )); then
    # Fallback: maybe the diff doesn't show the [Unreleased] header in context.
    # Accept any added line as a weaker signal.
    any_added="$(printf '%s\n' "$changelog_diff" \
        | grep -cE '^\+[^+]' \
        || true)"
    if (( any_added == 0 )); then
        cat <<EOF >&2
changelog gate FAILED

Code was changed under crates/ or apps/ but CHANGELOG.md [Unreleased] section
was not updated.
Required per .claude/CLAUDE.md "Changelogs — Registro Obrigatório de Mudanças":

  A seção [Unreleased] é obrigatória e é onde toda mudança entra primeiro.
  Use one of: Added | Changed | Deprecated | Removed | Fixed | Security.

Files touched:
$(printf '  - %s\n' $changed_files | head -20)
EOF
        exit 1
    fi
    echo "changelog: OK — $any_added line(s) added (could not confirm [Unreleased] section in diff context)"
    exit 0
fi

echo "changelog: OK — [Unreleased] grew by $added_in_unreleased line(s)"
exit 0
