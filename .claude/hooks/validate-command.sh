#!/bin/bash
# PreToolUse hook for Bash: validates dangerous commands
# Exit 0 = allow, Exit 2 = block (stderr shown to Claude)

set -euo pipefail

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if [ -z "$COMMAND" ]; then
  exit 0
fi

# Block destructive git operations
if echo "$COMMAND" | grep -qE 'git\s+(checkout|revert)\s'; then
  echo "BLOCKED: git checkout and git revert are forbidden. Use git stash or create a new branch." >&2
  exit 2
fi

if echo "$COMMAND" | grep -qE 'git\s+push\s+--force'; then
  echo "BLOCKED: force push is forbidden. Use --force-with-lease if absolutely necessary." >&2
  exit 2
fi

if echo "$COMMAND" | grep -qE 'git\s+reset\s+--hard'; then
  echo "BLOCKED: git reset --hard is forbidden. Use git stash instead." >&2
  exit 2
fi

# Block working directly on main
BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")
if [ "$BRANCH" = "main" ]; then
  if echo "$COMMAND" | grep -qE 'git\s+commit'; then
    echo "BLOCKED: Never commit directly to main. Create a feature branch first." >&2
    exit 2
  fi
fi

# Block rm -rf on project root or dangerous paths
if echo "$COMMAND" | grep -qE 'rm\s+-rf\s+(/|~|\.\s*$)'; then
  echo "BLOCKED: Dangerous rm -rf target detected." >&2
  exit 2
fi

# Block sudo
if echo "$COMMAND" | grep -qE '^\s*sudo\s'; then
  echo "BLOCKED: sudo is not allowed in this project." >&2
  exit 2
fi

exit 0
