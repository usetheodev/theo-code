# theo-governance — Revisao

> **Contexto**: Policy engine simplificado. Bounded Context: Governance. Sits in the critical path but lightweight.
>
> **Dependencias permitidas**: `theo-domain`.

## Dominios

| # | Nome | Descricao | Status |
|---|------|-----------|--------|
| 1 | `alerts` | Sistema de alertas quando politicas sao violadas/acionadas. | Pendente |
| 2 | `impact` | Avaliacao de impacto de acoes antes de permitir execucao. | Pendente |
| 3 | `metrics` | Metricas de governance (violacoes, approvals, denials). | Pendente |
| 4 | `sandbox_audit` | Auditoria de execucoes em sandbox (log + analise). | Pendente |
| 5 | `sandbox_policy` | Politicas de sandbox (bwrap/landlock/noop cascade). | Pendente |
| 6 | `sequence_analyzer` | Analise de sequencias de acoes (deteccao de padroes suspeitos). | Pendente |
