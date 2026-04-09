---
paths:
  - "crates/**/*.rs"
  - "apps/**/*.rs"
  - "apps/theo-ui/**/*.{ts,tsx}"
---

# TDD — Test-Driven Development (Inquebravel)

Todo codigo segue o ciclo RED-GREEN-REFACTOR. Sem excecoes.

## Ciclo Obrigatorio

```
1. RED    — Escreva o teste que FALHA (prova que o comportamento nao existe)
2. GREEN  — Escreva o MINIMO de codigo para o teste passar
3. REFACTOR — Limpe o codigo mantendo todos os testes verdes
```

## Regras

1. **Teste PRIMEIRO, codigo DEPOIS** — nunca escreva implementacao sem teste falhando
2. **Um teste por vez** — nao escreva 10 testes e depois implemente tudo
3. **Minimo para passar** — na fase GREEN, escreva o codigo mais simples possivel
4. **Refatore com seguranca** — so refatore quando todos os testes passam
5. **Teste comportamento, nao implementacao** — testes sobrevivem a refactors

## Fluxo Pratico (Rust)

```rust
// 1. RED — escreva o teste
#[test]
fn test_retrieval_returns_empty_for_unknown_query() {
    let engine = RetrievalEngine::new();
    let results = engine.search("nonexistent_xyz_123");
    assert!(results.is_empty());
}
// cargo test → FAIL (RetrievalEngine nao existe)

// 2. GREEN — implemente o minimo
pub struct RetrievalEngine;
impl RetrievalEngine {
    pub fn new() -> Self { Self }
    pub fn search(&self, _query: &str) -> Vec<SearchResult> { vec![] }
}
// cargo test → PASS

// 3. REFACTOR — melhore sem quebrar
// (neste caso, nada a refatorar ainda)
```

## Para Bug Fixes

```
1. Escreva o teste que REPRODUZ o bug (RED)
2. Confirme que o teste FALHA
3. Corrija o bug (GREEN)
4. Confirme que o teste PASSA
5. Refatore se necessario
```

## Para Agentes

- **Agentes que escrevem codigo**: DEVEM seguir RED-GREEN-REFACTOR
- **Agentes read-only (reviewers, validators)**: DEVEM verificar que TDD foi seguido
- **Skills que geram codigo**: DEVEM incluir etapa de teste no fluxo

## Verificacao

Antes de considerar qualquer tarefa completa:
```bash
cargo test -p <crate-afetado>  # TODOS os testes devem passar
```

Se algum teste falha → a tarefa NAO esta completa.
