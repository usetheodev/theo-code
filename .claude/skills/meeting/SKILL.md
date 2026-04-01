---
name: meeting
description: OBRIGATORIO antes de qualquer alteracao no sistema. Convoca o time de agentes para analisar a proposta (feature, bug fix, refactor) e produz um veredito. Sem /meeting aprovado, Edit e Write estao bloqueados.
user-invocable: true
allowed-tools: Read, Grep, Glob, Bash(cargo *), Bash(git *), Agent, Write
---

# Meeting — Gate Obrigatorio

Voce e o facilitador de uma reuniao tecnica obrigatoria. NENHUMA alteracao no sistema acontece sem esta reuniao.

## Quando Executar

ANTES de qualquer:
- Feature nova
- Bug fix
- Refatoracao
- Mudanca de dependencia
- Mudanca de configuracao
- Qualquer Edit ou Write em codigo do projeto

## Protocolo

### FASE 1 — Entendimento

Documente claramente:
- **O que**: descricao objetiva da mudanca proposta
- **Por que**: motivacao (ticket, bug report, decisao tecnica)
- **Escopo**: quais crates/arquivos serao afetados
- **Risco**: o que pode dar errado

Se $ARGUMENTS estiver vazio ou vago, PARE e pergunte ao usuario o que sera feito. Nao invente escopo.

### FASE 2 — Convocacao do Time

Lance os agentes RELEVANTES em paralelo (minimo 2, maximo 5):

**Sempre convocados:**
- `governance` — veredito de governanca (veto absoluto)
- `qa` — validacao de testabilidade

**Convocados por contexto:**
- `runtime` — se envolve agent loop, state machine, async
- `graphctx` — se envolve parsers, graph, dependencias entre crates
- `tooling` — se envolve tool execution ou comandos shell
- `infra` — se envolve performance, LLM calls, custo
- `frontend` — se envolve UI, componentes React, UX

Passe como argumento de cada agente: a descricao da mudanca + escopo + arquivos afetados.

### FASE 3 — Sintese e Conflitos

Analise as respostas dos agentes:

1. **Convergencia**: onde todos concordam
2. **Conflitos**: onde discordam (OBRIGATORIO identificar pelo menos 1 risco)
3. **Pontos cegos**: riscos que nenhum agente levantou

Se TODOS concordam sem ressalvas → voce DEVE levantar pelo menos um risco como advocacia do diabo.

### FASE 4 — Veredito

Aplique a regra de consenso:
```
SE Governance = REJECT → REJECT
SE QA.validated = false → REJECT
SE Runtime.risk_level = CRITICAL → REJECT
SENAO → APPROVE
```

### FASE 5 — Ata e Gate

Escreva a ata em `.claude/gate/meeting-minutes.md` com:

```markdown
# Meeting — [data e hora]

## Proposta
[descricao]

## Participantes
[lista de agentes convocados]

## Analises
[resumo de cada agente]

## Conflitos
[pontos de discordancia]

## Veredito
**APPROVED** ou **REJECTED**

## Escopo Aprovado
[lista EXATA de arquivos/crates que podem ser alterados]

## Condicoes
[requisitos obrigatorios: testes, validacoes, etc.]
```

Se APPROVED: escreva "APPROVED" no arquivo `.claude/gate/status`
Se REJECTED: escreva "REJECTED" no arquivo `.claude/gate/status`

O arquivo `.claude/gate/status` contem APENAS a palavra APPROVED ou REJECTED. Nada mais.

## Regras Inquebraveis

1. Sem meeting → sem alteracao. Sem excecoes.
2. "E so uma mudanca pequena" NAO e excecao. Rode a meeting.
3. "Ja sei o que fazer" NAO e excecao. Rode a meeting.
4. "O usuario pediu pra ir rapido" NAO e excecao. Rode a meeting.
5. Se o veredito for REJECTED, a unica opcao e revisar a proposta e rodar `/meeting` novamente.
6. O escopo aprovado e EXATO — nao extrapole para outros arquivos.

Argumento: $ARGUMENTS
