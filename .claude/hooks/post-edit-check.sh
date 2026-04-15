#!/bin/bash
# PostToolUse hook for Edit/Write: quick validation after file changes
# Runs cargo check on the affected crate for fast feedback
# Exit 0 = ok (stdout is additional context for Claude)

set -euo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // empty')

if [ -z "$FILE_PATH" ]; then
  exit 0
fi

# Only check Rust files
if ! echo "$FILE_PATH" | grep -qE '\.rs$'; then
  exit 0
fi

# Extract crate name
CRATE=""
if echo "$FILE_PATH" | grep -q "crates/"; then
  CRATE=$(echo "$FILE_PATH" | sed -n 's|.*crates/\([^/]*\)/.*|\1|p')
elif echo "$FILE_PATH" | grep -q "apps/theo-cli"; then
  CRATE="theo-cli"
elif echo "$FILE_PATH" | grep -q "apps/theo-desktop"; then
  CRATE="theo-desktop"
fi

if [ -n "$CRATE" ]; then
  # Quick type-check (faster than full build)
  if ! cargo check -p "$CRATE" --message-format=short 2>&1 | tail -5; then
    echo "cargo check failed for $CRATE — fix compilation errors before continuing."
  fi
fi

exit 0
