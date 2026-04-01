---
name: changelog
description: Use when asked to update the changelog, document recent changes, or prepare a release. Analyzes git log and classifies changes into Added/Changed/Fixed/Removed.
allowed-tools: Bash(git *), Read, Edit
---

## Atualizar Changelog

Analise as mudanças recentes e atualize o CHANGELOG.md.

### Passos:

1. Rode `git log --oneline -20` para ver commits recentes
2. Rode `git diff --stat HEAD~$ARGUMENTS` (ou HEAD~5 se sem argumento) para ver arquivos mudados
3. Leia o `CHANGELOG.md` atual
4. Classifique cada mudança em: Added, Changed, Fixed, Removed, Deprecated, Security
5. Adicione entradas na seção `[Unreleased]`

### Regras:
- Escreva para o consumidor, não para o desenvolvedor
- Uma linha por mudança
- Referência ao commit ou PR entre parênteses quando disponível
- NÃO inclua refatorações internas sem impacto externo
- NÃO use descrições vagas: "melhorias", "ajustes", "refatoração"

Diretório: `/home/paulo/Projetos/usetheo/theo-code/theo-code`

Argumento: $ARGUMENTS
