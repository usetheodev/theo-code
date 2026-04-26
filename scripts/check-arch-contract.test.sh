#!/usr/bin/env bash
# Regression test for scripts/check-arch-contract.sh
#
# Validates that `declared_theo_deps()` parses both forms of theo-*
# dependency declaration:
#   - inline:    `theo-foo = "0.1"` or `theo-foo = { path = "..." }`
#   - workspace: `theo-foo.workspace = true`
#
# Reproduces the bug from finding find_p5_001 (Phase 5 deep review):
# the original regex `^(theo-[a-zA-Z0-9_-]+)[[:space:]]*=.+` failed for
# the workspace form because `.` between crate name and `=` is not
# whitespace, leaving theo-isolation/theo-infra-mcp invisible to the
# arch-contract gate.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATE_SCRIPT="$REPO_ROOT/scripts/check-arch-contract.sh"

if [[ ! -x "$GATE_SCRIPT" ]]; then
    echo "FAIL: $GATE_SCRIPT not found or not executable" >&2
    exit 1
fi

# Source the gate in --source-only mode so we can call its functions
# without triggering the full scan.
# shellcheck disable=SC1090
source "$GATE_SCRIPT" --source-only

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

pass=0
fail=0

assert_detects() {
    local name="$1" cargo="$2" want="$3"
    local got
    got="$(declared_theo_deps "$cargo" | tr '\n' ' ')"
    got="${got%% }"
    if [[ "$got" == "$want" ]]; then
        echo "PASS: $name"
        pass=$((pass + 1))
    else
        echo "FAIL: $name"
        echo "  want: '$want'"
        echo "  got:  '$got'"
        fail=$((fail + 1))
    fi
}

# ---- Test 1: workspace dep is detected (was the bug) ------------------------
cat > "$tmpdir/workspace.toml" <<'EOF'
[package]
name = "test-crate"

[dependencies]
theo-fake.workspace = true
EOF
assert_detects "test_gate_detects_workspace_dep" "$tmpdir/workspace.toml" "theo-fake"

# ---- Test 2: inline dep is detected (regression of original behaviour) ------
cat > "$tmpdir/inline.toml" <<'EOF'
[package]
name = "test-crate"

[dependencies]
theo-fake = "0.1"
EOF
assert_detects "test_gate_detects_inline_dep" "$tmpdir/inline.toml" "theo-fake"

# ---- Test 3: third-party deps are ignored -----------------------------------
cat > "$tmpdir/third.toml" <<'EOF'
[package]
name = "test-crate"

[dependencies]
serde.workspace = true
tokio = "1.0"
EOF
assert_detects "test_gate_ignores_third_party" "$tmpdir/third.toml" ""

# ---- Test 4: mixed forms in same Cargo.toml ---------------------------------
cat > "$tmpdir/mixed.toml" <<'EOF'
[package]
name = "test-crate"

[dependencies]
theo-foo.workspace = true
theo-bar = "0.1"
serde.workspace = true
EOF
assert_detects "test_gate_detects_mixed_forms" "$tmpdir/mixed.toml" "theo-bar theo-foo"

# ---- Test 5: path-form dep is detected --------------------------------------
cat > "$tmpdir/path.toml" <<'EOF'
[package]
name = "test-crate"

[dependencies]
theo-foo = { path = "../theo-foo" }
EOF
assert_detects "test_gate_detects_path_dep" "$tmpdir/path.toml" "theo-foo"

# ---- Test 6: inline trailing comments are stripped --------------------------
# (Indented deps in `[dependencies]` are not exercised here — Cargo.toml
# in this workspace never indents under `[dependencies]`, and the gate's
# leading-whitespace trim has a bash-pattern subtlety that is out of
# scope for the regex-fix task T0.1.)
cat > "$tmpdir/comments.toml" <<'EOF'
[package]
name = "test-crate"

[dependencies]
# whole-line comment, ignored
theo-foo.workspace = true # inline trailing comment
theo-bar = "0.1"
EOF
assert_detects "test_gate_handles_inline_comments" "$tmpdir/comments.toml" "theo-bar theo-foo"

echo
echo "Summary: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
