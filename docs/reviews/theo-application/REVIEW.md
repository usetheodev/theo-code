# theo-application — Revisao

> **Contexto**: Camada de use cases. Unica camada que pode depender de todos os crates. Apps (`theo-cli`, `theo-desktop`) consomem APENAS esta camada + `theo-api-contracts`.
>
> **Papel**: orquestra fluxos cross-crate sem vazar dependencias internas para os apps.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `facade` | Fachada unificada de alto nivel exposta aos apps. | Pendente |
| 2 | `use_cases::agents_dashboard` | Use case: agregar dados para dashboard de agents. | Pendente |
| 3 | `use_cases::auth` | Use case: fluxos de autenticacao (login, logout, refresh). | Pendente |
| 4 | `use_cases::context_assembler` | Use case: assembly de contexto de codigo + memoria + working set. | Pendente |
| 5 | `use_cases::conversion` | Use case: conversao entre formatos (DTO ↔ domain). | Pendente |
| 6 | `use_cases::extraction` | Use case: extracao de informacoes de artefatos. | Pendente |
| 7 | `use_cases::graph_context_service` | Use case: servico de consulta ao grafo de codigo. | Pendente |
| 8 | `use_cases::guardrail_loader` | Use case: carregamento de guardrails do projeto. | Pendente |
| 9 | `use_cases::impact` | Use case: analise de impacto (o que uma mudanca afeta). | Pendente |
| 10 | `use_cases::memory_factory` | Use case: factory de `MemoryEngine` configurado. | Pendente |
| 11 | `use_cases::memory_lint` | Use case: lint de entradas de memoria. | Pendente |
