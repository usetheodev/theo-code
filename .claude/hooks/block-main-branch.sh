#!/bin/bash
# Bloqueia commits e operações destrutivas direto na main
set -euo pipefail

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // ""')

# Detecta operações na branch main
if echo "$COMMAND" | grep -qE 'git (commit|merge|rebase|cherry-pick)'; then
  BRANCH=$(git -C /home/paulo/theo-code branch --show-current 2>/dev/null || echo "")
  if [ "$BRANCH" = "main" ]; then
    echo "BLOQUEADO: operação git na branch main. Crie uma branch primeiro." >&2
    exit 2
  fi
fi

# Detecta checkout e revert (proibidos pelo CLAUDE.md global)
if echo "$COMMAND" | grep -qE 'git (checkout|revert)'; then
  echo "BLOQUEADO: git checkout/revert são proibidos. Use alternativas seguras." >&2
  exit 2
fi

exit 0
