#!/bin/bash
# PreToolUse hook for Edit/Write: checks architectural boundaries
# Validates that files being edited don't violate dependency rules
# Exit 0 = allow, Exit 2 = block

set -euo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // empty')

if [ -z "$FILE_PATH" ]; then
  exit 0
fi

# Extract crate name from path
CRATE=""
if echo "$FILE_PATH" | grep -q "crates/"; then
  CRATE=$(echo "$FILE_PATH" | sed -n 's|.*crates/\([^/]*\)/.*|\1|p')
fi

# If editing a Cargo.toml in a crate, validate dependencies
if echo "$FILE_PATH" | grep -qE 'crates/[^/]+/Cargo.toml$'; then
  # theo-domain must have zero internal dependencies
  if [ "$CRATE" = "theo-domain" ]; then
    if echo "$INPUT" | jq -r '.tool_input.new_string // .tool_input.content // empty' | grep -qE 'path\s*=\s*"\.\./theo-'; then
      echo '{"decision":"block","reason":"BOUNDARY VIOLATION: theo-domain must have ZERO dependencies on other theo crates. It is the pure types layer."}' >&2
      exit 2
    fi
  fi
fi

# Warn if editing app code that imports engine crates directly
if echo "$FILE_PATH" | grep -qE 'apps/(theo-cli|theo-desktop)/.*\.rs$'; then
  CONTENT=$(echo "$INPUT" | jq -r '.tool_input.new_string // .tool_input.content // empty')
  if echo "$CONTENT" | grep -qE 'use\s+theo_(engine|infra|tooling|governance)'; then
    echo '{"decision":"block","reason":"BOUNDARY VIOLATION: Apps must not import engine/infra/tooling/governance crates directly. Use theo-application as the intermediary."}' >&2
    exit 2
  fi
fi

exit 0
