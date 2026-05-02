#!/usr/bin/env bash
# T0.2 regression tests for scripts/extract-tests-to-sibling.py.
#
# Creates fixture .rs files in a tempdir, runs the extractor, asserts
# the output matches expected. 4 fixtures cover the contract:
#   1. Simple inline mod tests
#   2. Idempotency (already-extracted file → no-op)
#   3. Raw string containing `#[cfg(test)]` (false-positive guard)
#   4. Header / imports preserved

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SCRIPT="$REPO_ROOT/scripts/extract-tests-to-sibling.py"

WORKDIR="$(mktemp -d)"
trap "rm -rf '$WORKDIR'" EXIT

PASS=0
FAIL=0

assert_contains() {
    local file="$1" needle="$2" label="$3"
    if grep -qF "$needle" "$file"; then
        echo "PASS $label"
        PASS=$((PASS + 1))
    else
        echo "FAIL $label — '$needle' not in $file"
        FAIL=$((FAIL + 1))
    fi
}

assert_not_contains() {
    local file="$1" needle="$2" label="$3"
    if ! grep -qF "$needle" "$file"; then
        echo "PASS $label"
        PASS=$((PASS + 1))
    else
        echo "FAIL $label — '$needle' SHOULD NOT be in $file"
        FAIL=$((FAIL + 1))
    fi
}

# ── Fixture 1: simple inline mod tests ────────────────────────────────────
F1="$WORKDIR/simple.rs"
cat > "$F1" <<'EOF'
pub fn answer() -> u32 { 42 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_returns_42() {
        assert_eq!(answer(), 42);
    }
}
EOF

python3 "$SCRIPT" "$F1" > /dev/null
assert_contains "$F1" '#[path = "simple_tests.rs"]'                "F1: original uses path-form"
assert_not_contains "$F1" 'mod tests {'                            "F1: original lost inline mod"
assert_contains "$WORKDIR/simple_tests.rs" 'fn it_returns_42'      "F1: sibling has the test"
assert_contains "$WORKDIR/simple_tests.rs" 'use super::*;'         "F1: sibling has inner imports"

# ── Fixture 2: idempotent (already extracted) ─────────────────────────────
F2="$WORKDIR/already.rs"
cat > "$F2" <<'EOF'
pub fn n() -> u32 { 1 }

#[cfg(test)]
#[path = "already_tests.rs"]
mod tests;
EOF

# Capture output (must say no extraction needed)
OUTPUT=$(python3 "$SCRIPT" "$F2")
if echo "$OUTPUT" | grep -q 'no extraction needed'; then
    echo "PASS F2: idempotent — already-extracted file produces no-op"
    PASS=$((PASS + 1))
else
    echo "FAIL F2: idempotent — got: $OUTPUT"
    FAIL=$((FAIL + 1))
fi

# ── Fixture 3: raw string with #[cfg(test)] (false-positive guard) ─────────
F3="$WORKDIR/raw.rs"
cat > "$F3" <<'EOF'
pub const TEMPLATE: &str = r#"
#[cfg(test)]
mod tests {
    fn embedded() {}
}
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert!(true);
    }
}
EOF

python3 "$SCRIPT" "$F3" > /dev/null
# Real test mod must have been extracted; the embedded raw-string content stays.
assert_contains "$F3" 'pub const TEMPLATE'                         "F3: raw-string const preserved"
assert_contains "$F3" '#[path = "raw_tests.rs"]'                   "F3: real test mod was extracted"
assert_contains "$WORKDIR/raw_tests.rs" 'fn it_works'              "F3: sibling has the real test"
# CRITICAL — the false-positive #[cfg(test)] inside the raw string must NOT
# end up in the sibling.
if grep -q 'fn embedded' "$WORKDIR/raw_tests.rs"; then
    echo "FAIL F3: false positive — embedded fn in raw string ended up in sibling"
    FAIL=$((FAIL + 1))
else
    echo "PASS F3: no false positive on raw-string #[cfg(test)]"
    PASS=$((PASS + 1))
fi

# ── Fixture 4: file header + imports preserved ────────────────────────────
F4="$WORKDIR/header.rs"
cat > "$F4" <<'EOF'
//! This is the file header documentation.
//! Multiple lines.

use std::collections::HashMap;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_deserializes() {
        let _ = Config { key: "x".into() };
    }
}
EOF

python3 "$SCRIPT" "$F4" > /dev/null
assert_contains "$F4" '//! This is the file header documentation.'  "F4: header preserved"
assert_contains "$F4" 'use std::collections::HashMap;'              "F4: imports preserved"
assert_contains "$F4" 'pub struct Config'                           "F4: production code preserved"
assert_contains "$F4" '#[path = "header_tests.rs"]'                 "F4: tests extracted"
assert_contains "$WORKDIR/header_tests.rs" 'fn config_deserializes' "F4: sibling has test"

# ── Summary ───────────────────────────────────────────────────────────────
echo ""
echo "Result: $PASS passed, $FAIL failed"
exit "$FAIL"
