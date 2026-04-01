#!/bin/bash
# Após editar arquivos Rust, verifica se compila
set -euo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // ""')

# Só verifica arquivos Rust no workspace
if [[ "$FILE_PATH" == *.rs ]] && [[ "$FILE_PATH" == *theo-code* ]]; then
  # Extrai o nome do crate pelo path
  CRATE_NAME=""
  if [[ "$FILE_PATH" =~ crates/([^/]+)/ ]]; then
    CRATE_NAME="${BASH_REMATCH[1]}"
  elif [[ "$FILE_PATH" =~ apps/([^/]+)/ ]]; then
    CRATE_NAME="${BASH_REMATCH[1]}"
  fi

  if [ -n "$CRATE_NAME" ]; then
    cd /home/paulo/Projetos/usetheo/theo-code/theo-code
    # Check rápido — só verifica se compila, sem rodar testes
    if ! cargo check -p "$CRATE_NAME" --quiet 2>/dev/null; then
      echo "AVISO: cargo check falhou para $CRATE_NAME após edição de $FILE_PATH" >&2
    fi
  fi
fi

exit 0
