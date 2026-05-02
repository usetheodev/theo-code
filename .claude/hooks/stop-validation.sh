#!/bin/bash
# Stop hook: validates that modified crates have passing tests and features are wired.
# Any stdout output is fed back to Claude as context, preventing premature stop.
# If no issues are found, output is empty and Claude stops normally.

set -uo pipefail

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-.}"
cd "$PROJECT_DIR"

# ---------------------------------------------------------------------------
# 1. Collect ALL modified files (unstaged + staged + last commit)
# ---------------------------------------------------------------------------
UNSTAGED=$(git diff --name-only 2>/dev/null || true)
STAGED=$(git diff --cached --name-only 2>/dev/null || true)
LAST_COMMIT=$(git diff --name-only HEAD~1..HEAD 2>/dev/null || true)

ALL_FILES=$(echo -e "${UNSTAGED}\n${STAGED}\n${LAST_COMMIT}" | sort -u | grep -v '^$' || true)

if [ -z "$ALL_FILES" ]; then
  exit 0
fi

# Only care about source files
RUST_CHANGED=$(echo "$ALL_FILES" | grep -E '\.rs$' || true)
TS_CHANGED=$(echo "$ALL_FILES" | grep -E '\.(ts|tsx)$' || true)

if [ -z "$RUST_CHANGED" ] && [ -z "$TS_CHANGED" ]; then
  exit 0
fi

ISSUES=()
TESTED_CRATES=()
WARNINGS=()

# ---------------------------------------------------------------------------
# 2. Extract affected Rust crates
# ---------------------------------------------------------------------------
CRATES=""
if [ -n "$RUST_CHANGED" ]; then
  LIB_CRATES=$(echo "$RUST_CHANGED" | sed -n 's|^crates/\([^/]*\)/.*|\1|p' | sort -u || true)
  APP_CRATES=$(echo "$RUST_CHANGED" | sed -n 's|^apps/\(theo-cli\|theo-desktop\|theo-marklive\)/.*|\1|p' | sort -u || true)
  CRATES=$(echo -e "${LIB_CRATES}\n${APP_CRATES}" | grep -v '^$' | sort -u || true)
fi

# ---------------------------------------------------------------------------
# 3. Run tests for each affected crate (cap at 5 to keep it fast)
# ---------------------------------------------------------------------------
CRATE_COUNT=0
if [ -n "$CRATES" ]; then
  while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    CRATE_COUNT=$((CRATE_COUNT + 1))
    if [ "$CRATE_COUNT" -gt 5 ]; then
      WARNINGS+=("More than 5 crates changed — tested only the first 5. Run 'cargo test --workspace' manually.")
      break
    fi

    TESTED_CRATES+=("$crate")
    TEST_OUTPUT=$(cargo test -p "$crate" --lib --tests 2>&1 || true)

    if echo "$TEST_OUTPUT" | grep -qE 'FAILED|error\[E'; then
      FAILURES=$(echo "$TEST_OUTPUT" | grep -E 'FAILED|error\[E|panicked|test result:' | head -10)
      ISSUES+=("TESTS FAILING in $crate:\n$FAILURES")
    fi
  done <<< "$CRATES"
fi

# ---------------------------------------------------------------------------
# 4. Always test theo-application if any lower crate changed
#    (this is the integration boundary — catches unwired features)
# ---------------------------------------------------------------------------
if [ -n "$LIB_CRATES" ] 2>/dev/null; then
  ALREADY_TESTED=false
  for t in "${TESTED_CRATES[@]+"${TESTED_CRATES[@]}"}"; do
    [ "$t" = "theo-application" ] && ALREADY_TESTED=true
  done

  if [ "$ALREADY_TESTED" = false ]; then
    TESTED_CRATES+=("theo-application")
    APP_OUTPUT=$(cargo test -p theo-application --lib --tests 2>&1 || true)

    if echo "$APP_OUTPUT" | grep -qE 'FAILED|error\[E'; then
      FAILURES=$(echo "$APP_OUTPUT" | grep -E 'FAILED|error\[E|panicked|test result:' | head -10)
      ISSUES+=("INTEGRATION BOUNDARY TESTS FAILING in theo-application:\n$FAILURES")
    fi
  fi
fi

# ---------------------------------------------------------------------------
# 5. Check for orphaned public APIs (new pub items referenced nowhere else)
# ---------------------------------------------------------------------------
if [ -n "$RUST_CHANGED" ]; then
  # Get newly added pub items from the diff
  NEW_PUB=$(git diff HEAD -- '*.rs' 2>/dev/null | \
    grep -E '^\+\s*pub\s+(fn|struct|enum|trait)\s+' | \
    sed 's/^+\s*//' || true)

  # Also check staged diff
  STAGED_PUB=$(git diff --cached -- '*.rs' 2>/dev/null | \
    grep -E '^\+\s*pub\s+(fn|struct|enum|trait)\s+' | \
    sed 's/^+\s*//' || true)

  ALL_PUB=$(echo -e "${NEW_PUB}\n${STAGED_PUB}" | sort -u | grep -v '^$' || true)

  if [ -n "$ALL_PUB" ]; then
    while IFS= read -r line; do
      [ -z "$line" ] && continue

      ITEM_NAME=$(echo "$line" | grep -oP '(fn|struct|enum|trait)\s+\K\w+' || true)
      [ -z "$ITEM_NAME" ] && continue

      # Skip common/trivial names that are always legitimate
      if echo "$ITEM_NAME" | grep -qiE '^(test_|new|default|from|into|fmt|display|error|debug|clone|drop|build|with_|try_|is_|as_|get_|set_|has_|to_|len|iter)'; then
        continue
      fi

      # Skip short names (likely generic helpers)
      if [ ${#ITEM_NAME} -le 3 ]; then
        continue
      fi

      # Count files referencing this item across the workspace
      REF_COUNT=$(grep -rl "\b${ITEM_NAME}\b" crates/ apps/ --include='*.rs' 2>/dev/null | wc -l || echo "0")

      if [ "$REF_COUNT" -le 1 ]; then
        ITEM_TYPE=$(echo "$line" | grep -oP 'pub\s+\K(fn|struct|enum|trait)' || echo "item")
        WARNINGS+=("POSSIBLY UNWIRED: pub $ITEM_TYPE '$ITEM_NAME' found in only $REF_COUNT file(s). Is it integrated into the system?")
      fi
    done <<< "$ALL_PUB"
  fi
fi

# ---------------------------------------------------------------------------
# 6. Check for new tool files not registered in DefaultRegistry
# ---------------------------------------------------------------------------
if echo "$RUST_CHANGED" | grep -q "crates/theo-tooling/src"; then
  # If tooling was changed, verify registry builds
  REGISTRY_OUTPUT=$(cargo test -p theo-tooling build_registry 2>&1 || true)
  if echo "$REGISTRY_OUTPUT" | grep -qE 'FAILED|error\[E'; then
    ISSUES+=("TOOL REGISTRY BROKEN: 'build_registry' test failed after tooling changes.\nNew tools must be registered in DefaultRegistry.")
  fi
fi

# ---------------------------------------------------------------------------
# 7. Run frontend tests if TS changed
# ---------------------------------------------------------------------------
if [ -n "$TS_CHANGED" ]; then
  if [ -d "$PROJECT_DIR/apps/theo-ui" ]; then
    TESTED_CRATES+=("theo-ui")
    UI_OUTPUT=$(cd "$PROJECT_DIR/apps/theo-ui" && npm test -- --run 2>&1 || true)

    if echo "$UI_OUTPUT" | grep -qiE 'FAIL|failed|error'; then
      UI_FAILURES=$(echo "$UI_OUTPUT" | grep -iE 'FAIL|failed|error|✗|×' | head -10)
      ISSUES+=("UI TESTS FAILING:\n$UI_FAILURES")
    fi
  fi
fi

# ---------------------------------------------------------------------------
# 8. Report
# ---------------------------------------------------------------------------
HAS_OUTPUT=false

if [ ${#ISSUES[@]} -gt 0 ]; then
  HAS_OUTPUT=true
  echo "============================================"
  echo "  INTEGRATION VALIDATION FAILED"
  echo "============================================"
  echo ""
  for issue in "${ISSUES[@]}"; do
    echo -e "  [BLOCK] $issue"
    echo ""
  done
fi

if [ ${#WARNINGS[@]} -gt 0 ]; then
  HAS_OUTPUT=true
  if [ ${#ISSUES[@]} -eq 0 ]; then
    echo "============================================"
    echo "  INTEGRATION VALIDATION — WARNINGS"
    echo "============================================"
    echo ""
  fi
  for warning in "${WARNINGS[@]}"; do
    echo -e "  [WARN] $warning"
    echo ""
  done
fi

if [ "$HAS_OUTPUT" = true ]; then
  echo "--------------------------------------------"
  echo "Crates tested: ${TESTED_CRATES[*]}"
  echo ""
  if [ ${#ISSUES[@]} -gt 0 ]; then
    echo "ACTION REQUIRED: Fix the [BLOCK] issues above before finishing."
    echo "Run 'cargo test -p <crate>' to verify fixes."
  else
    echo "No blocking issues. Warnings are advisory — verify they are intentional."
  fi
fi

exit 0
