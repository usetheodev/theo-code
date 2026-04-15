# ADR-004: Interactive Approval Gate

**Status:** Aceito
**Data:** 2026-04-15
**Autor:** Paulo (multi-agent meeting — governance, runtime, qa, frontend, tooling)
**Escopo:** theo-domain (trait), theo-agent-runtime (ToolCallManager integration), theo-cli (TUI modal)

---

## Contexto

O runtime atual usa `CapabilityGate` (`capability_gate.rs`) para controle de acesso a tools — check binario allow/deny sem interacao humana. Para governance visual no TUI, precisamos de um handshake interativo: o runtime pausa execucao, a TUI mostra um modal de aprovacao, o usuario decide, e o runtime continua.

### Problema

```
Hoje:     ToolCallManager → CapabilityGate.check_tool() → Ok/Err → executa/rejeita
Queremos: ToolCallManager → ApprovalGate.request_approval() → pausa → TUI modal → usuario → resume
```

Nenhum mecanismo de handshake bidirecional existe entre runtime e TUI.

---

## Decisao

Implementar trait `ApprovalGate` com protocolo baseado em `tokio::sync::oneshot` channel.

### Principios

1. **CapabilityGate intacto** — ApprovalGate e camada adicional, nao substitutiva
2. **Composicao** — CapabilityGate roda primeiro (deny rapido), ApprovalGate so para tools que passam
3. **Async-safe** — runtime pausa via `.await` no oneshot, nao bloqueia threads
4. **Fallback** — `AutoApproveGate` para CLI legado e testes (zero friction)
5. **Timeout** — auto-reject apos 5 minutos se TUI nao responder

---

## Arquitetura

```
ToolCallManager::dispatch_and_execute()
  │
  ├── CapabilityGate.check_tool() ──── deny → Err (fast path, unchanged)
  │
  ├── ApprovalGate.request_approval() ── deny → Err (interactive)
  │     │
  │     ├── Publica GovernanceDecisionPending no EventBus
  │     ├── Cria oneshot::channel()
  │     ├── Registra pending_decision no ApprovalGate
  │     ├── .await no oneshot::Receiver (runtime pausa aqui)
  │     │
  │     │   [TUI recebe GovernanceDecisionPending via broadcast]
  │     │   [TUI mostra modal: tool_name, risk, approve/reject]
  │     │   [Usuario pressiona a/r]
  │     │   [TUI chama approval_gate.resolve(decision_id, outcome)]
  │     │   [oneshot::Sender envia outcome]
  │     │
  │     └── Recebe outcome → Approved/Rejected
  │
  └── tool_bridge::execute_tool_call() ── executa normalmente
```

### Trait

```rust
#[async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalOutcome;
}

pub struct ApprovalRequest {
    pub decision_id: String,
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub risk_level: RiskLevel,
}

pub enum ApprovalOutcome {
    Approved,
    Rejected(String),
    Timeout,
}

pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}
```

### Implementacoes

1. **TuiApprovalGate** — registra oneshot sender, publica evento, aguarda resposta
2. **AutoApproveGate** — retorna `Approved` imediatamente (para CLI legado e testes)

### Eventos

- `GovernanceDecisionPending` — payload: `{ decision_id, tool_name, risk_level, args_preview }`
- `GovernanceDecisionResolved` — payload: `{ decision_id, outcome }`

### Integracao no ToolCallManager

Em `dispatch_and_execute()`, entre CapabilityGate check e tool execution:

```rust
// After capability check passes
if let Some(gate) = &self.approval_gate {
    let request = ApprovalRequest { ... };
    match gate.request_approval(request).await {
        ApprovalOutcome::Approved => { /* continue */ }
        ApprovalOutcome::Rejected(reason) => { return Err(...) }
        ApprovalOutcome::Timeout => { return Err(...) }
    }
}
```

### Risk Assessment

Cada tool tem um risk level baseado na categoria:
- **Low**: Read, Glob, Grep, Think, Reflect
- **Medium**: Write, Edit, ApplyPatch
- **High**: Bash, WebFetch
- **Critical**: nenhuma por default (configuravel)

Tools de risk Low sao auto-approved (nao mostram modal).

---

## Riscos

1. **Deadlock** se TUI fechar sem resolver pending decision — mitigado por timeout 5min
2. **Race condition** se multiplas tool calls pendentes — cada uma tem decision_id unico
3. **Performance** — oneshot await e O(1), nao afeta throughput

---

## Testes

- `auto_approve_gate_approves_all` — AutoApproveGate retorna Approved
- `tui_approval_gate_approve_flow` — request → resolve(Approved) → retorna Approved
- `tui_approval_gate_reject_flow` — request → resolve(Rejected) → retorna Rejected
- `tui_approval_gate_timeout` — request → 5min sem resposta → Timeout
