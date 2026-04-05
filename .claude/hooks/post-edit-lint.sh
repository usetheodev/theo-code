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

if echo "$CHECK_OUTPUT" | grep -q "^error"; then
  # Parse errors and produce LLM-optimized remediation messages.
  # Each message includes: ERROR line, FILE:LINE, FIX instruction, VERIFY command.
  echo "" >&2
  echo "=== COMPILATION ERROR in crate '$CRATE_NAME' ===" >&2
  echo "EDITED: $FILE_PATH" >&2
  echo "" >&2

  echo "$CHECK_OUTPUT" | grep "^error" | head -8 | while IFS= read -r LINE; do
    echo "ERROR: $LINE" >&2

    # Extract file:line if present in error message
    ERR_LOC=$(echo "$LINE" | grep -oP '[a-zA-Z_/]+\.rs:\d+' | head -1 || true)
    if [ -n "$ERR_LOC" ]; then
      echo "  AT: $ERR_LOC" >&2
    fi

    # LLM-optimized remediation hints with specific next actions
    if echo "$LINE" | grep -q "unresolved import"; then
      MODULE=$(echo "$LINE" | grep -oP '`[^`]+`' | head -1 || true)
      echo "  FIX: Add the missing 'use' import for $MODULE at the top of the file." >&2
      echo "  HINT: Run 'grep -rn \"pub.*${MODULE//\`/}\" crates/' to find where it's defined." >&2
    elif echo "$LINE" | grep -q "cannot find"; then
      IDENT=$(echo "$LINE" | grep -oP '`[^`]+`' | head -1 || true)
      echo "  FIX: Check spelling of $IDENT, or add a 'use' import." >&2
      echo "  HINT: Run 'grep -rn \"pub.*${IDENT//\`/}\" crates/' to find the definition." >&2
    elif echo "$LINE" | grep -q "missing field"; then
      FIELD=$(echo "$LINE" | grep -oP '`[^`]+`' | head -1 || true)
      echo "  FIX: Add the missing field $FIELD to the struct literal." >&2
      echo "  HINT: Read the struct definition to see all required fields." >&2
    elif echo "$LINE" | grep -q "cannot borrow.*as mutable"; then
      echo "  FIX: Change '&self' to '&mut self' in the method signature, or use interior mutability." >&2
    elif echo "$LINE" | grep -q "expected.*found"; then
      echo "  FIX: Type mismatch — check the return type or variable type." >&2
    elif echo "$LINE" | grep -q "unused variable"; then
      echo "  FIX: Prefix with underscore (_var) or remove the variable." >&2
    elif echo "$LINE" | grep -q "no method named"; then
      METHOD=$(echo "$LINE" | grep -oP '`[^`]+`' | head -1 || true)
      echo "  FIX: Method $METHOD does not exist on this type. Check the type's impl block." >&2
    elif echo "$LINE" | grep -q "trait bound.*is not satisfied"; then
      echo "  FIX: The type does not implement the required trait. Add #[derive(...)] or impl the trait." >&2
    fi

    echo "" >&2
  done

  echo "VERIFY: Run 'cargo check -p $CRATE_NAME' after fixing." >&2
  echo "===" >&2
fi

exit 0
