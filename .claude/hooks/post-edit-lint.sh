#!/bin/bash
# Post-edit sensor: after editing Rust files, check compilation and provide
# LLM-optimized remediation hints for common errors.
set -euo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // ""')

# Only check Rust files in the workspace
if [[ "$FILE_PATH" != *.rs ]]; then
  exit 0
fi

# Find workspace root (walk up to find Cargo.toml with [workspace])
WORKSPACE_ROOT=""
DIR=$(dirname "$FILE_PATH")
while [ "$DIR" != "/" ] && [ "$DIR" != "." ]; do
  if [ -f "$DIR/Cargo.toml" ] && grep -q '\[workspace\]' "$DIR/Cargo.toml" 2>/dev/null; then
    WORKSPACE_ROOT="$DIR"
    break
  fi
  DIR=$(dirname "$DIR")
done

if [ -z "$WORKSPACE_ROOT" ]; then
  exit 0
fi

# Extract crate name from path
CRATE_NAME=""
if [[ "$FILE_PATH" =~ crates/([^/]+)/ ]]; then
  CRATE_NAME="${BASH_REMATCH[1]}"
elif [[ "$FILE_PATH" =~ apps/([^/]+)/ ]]; then
  CRATE_NAME="${BASH_REMATCH[1]}"
fi

if [ -z "$CRATE_NAME" ]; then
  exit 0
fi

cd "$WORKSPACE_ROOT"

# Run cargo check and capture errors (NOT --quiet — we need the messages)
CHECK_OUTPUT=$(cargo check -p "$CRATE_NAME" --message-format=short 2>&1) || true

if [ $? -ne 0 ] || echo "$CHECK_OUTPUT" | grep -q "^error"; then
  # Parse common errors and add LLM remediation hints
  echo "COMPILATION ERROR in $CRATE_NAME after editing $FILE_PATH:" >&2
  echo "$CHECK_OUTPUT" | grep "^error" | head -5 | while IFS= read -r LINE; do
    echo "  $LINE" >&2

    # Remediation hints for common patterns
    if echo "$LINE" | grep -q "unresolved import"; then
      echo "  FIX: Add the missing 'use' import at the top of the file." >&2
    elif echo "$LINE" | grep -q "cannot find"; then
      echo "  FIX: Check spelling of the identifier, or add a 'use' import." >&2
    elif echo "$LINE" | grep -q "missing field"; then
      echo "  FIX: Add the missing field to the struct literal." >&2
    elif echo "$LINE" | grep -q "cannot borrow.*as mutable"; then
      echo "  FIX: Change '&self' to '&mut self', or use interior mutability (Cell/RefCell/Mutex)." >&2
    elif echo "$LINE" | grep -q "expected.*found"; then
      echo "  FIX: Check the return type or variable type — there is a type mismatch." >&2
    elif echo "$LINE" | grep -q "unused variable"; then
      echo "  FIX: Prefix the variable with underscore (_var) or remove it." >&2
    fi
  done
fi

exit 0
