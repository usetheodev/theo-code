#!/bin/bash
# MEETING GATE — Bloqueia Edit/Write sem meeting aprovada
set -euo pipefail

GATE_DIR="/home/paulo/Projetos/usetheo/theo-code/.claude/gate"
STATUS_FILE="$GATE_DIR/status"

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // ""')
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // ""')

# Permite edições em arquivos do .claude/ (configuração, atas, gate)
if [[ "$FILE_PATH" == *".claude/"* ]]; then
  exit 0
fi

# Permite edições em arquivos fora do workspace theo-code (ex: memory)
if [[ "$FILE_PATH" != *"theo-code/"* ]]; then
  exit 0
fi

# Verifica se existe arquivo de status
if [ ! -f "$STATUS_FILE" ]; then
  echo "BLOQUEADO: Nenhuma meeting foi realizada. Execute /meeting antes de alterar o sistema." >&2
  exit 2
fi

# Verifica se o status é APPROVED
STATUS=$(cat "$STATUS_FILE" 2>/dev/null || echo "")
if [ "$STATUS" != "APPROVED" ]; then
  echo "BLOQUEADO: Meeting resultou em REJECTED. Revise a proposta e execute /meeting novamente." >&2
  exit 2
fi

exit 0
