---
name: consensus
description: Consensus engine rapido — coleta vereditos de Governance + QA + Runtime e decide APPROVE/REJECT sem debate completo
user-invocable: true
allowed-tools: Read, Grep, Glob, Bash(cargo *), Bash(git *), Agent
---

## Consensus Engine — Decisao Rapida

Versao rapida do review-council para decisoes que nao precisam de debate completo.

### Para "$ARGUMENTS":

**PASSO 1 — Coleta rapida de evidencias**

```bash
git diff --stat
cargo check 2>&1 | tail -5
cargo test 2>&1 | tail -10
```

**PASSO 2 — Lancamento paralelo dos 3 agentes core**

Lance em paralelo:
1. Agent `governance` — veredito de governanca
2. Agent `qa` — validacao de testes
3. Agent `runtime` — risco de execucao

**PASSO 3 — Aplicar regra de decisao**

```
SE Governance.decision = REJECT → REJECT
SE QA.validated = false → REJECT
SE Runtime.risk_level >= HIGH → REJECT
SENAO → APPROVE
```

**PASSO 4 — Report conciso**

```
CONSENSUS: APPROVE/REJECT
  Governance: APPROVE/REJECT (confidence: X%)
  QA: PASS/FAIL (X tests passed, Y failed)
  Runtime: LOW/MEDIUM/HIGH/CRITICAL
  Razao: [1 frase]
```

Nao faca debate. Nao faca advocacia do diabo. Apenas colete, aplique a regra, e reporte.
