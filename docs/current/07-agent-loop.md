# 07 — Agent Loop

O loop principal async que orquestra LLM calls, tool execution, e governance. Toda tool call passa pelo Decision Control Plane antes de ser executada.

**Depende de**: [05-validation-pipeline.md](05-validation-pipeline.md), [06-governance-layer.md](06-governance-layer.md), [08-llm-client.md](08-llm-client.md), [09-promise-system.md](09-promise-system.md), [10-context-loop-and-decomposer.md](10-context-loop-and-decomposer.md)

---

## Agent + AgentBuilder

```
crates/agent/src/
  agent.rs                   # Agent + AgentBuilder (main loop)
  config.rs                  # AgentConfig
  phase.rs                   # Phase enum + AgentState + transicoes
```

---

## Fases do Agent

O agent progride por fases deterministicas (State Machine):

```
LOCATE → EDIT → VERIFY → (CORRECT?) → DONE
```

Transicoes de fase sao governadas — cada transicao gera uma `DecisionType::PhaseTransition` que passa pela ValidationPipeline.

---

## run_loop() — Main async loop

```rust
impl Agent {
    async fn run_loop(&self, repo: &Path, task: &str, scope: &ScopedContext) -> Result<LoopResult> {
        let mut state = AgentState::new(scope.clone());
        let ctx_engine = ContextLoopEngine::new(self.config.max_iterations);

        for i in 1..=self.config.max_iterations {
            // ── Context Loop injection ──
            if let Some(ctx_msg) = ctx_engine.maybe_emit(&state, task) {
                self.history.push(user_msg(&ctx_msg));
            }

            // ── Phase transition ──
            if let Some(transition) = state.should_transition(&self.config.phase_config) {
                let decision = self.propose_decision(DecisionType::PhaseTransition, &state);
                let result = self.governance.validate(&decision);
                if result.verdict == Verdict::Allow {
                    state.transition_to(transition);
                    self.governance.record(decision.approve().activate().complete());
                }
            }

            // ── LLM call ──
            let response = self.llm.complete(self.build_request(&state)).await?;

            // ── Process tool calls ──
            for tool_call in response.tool_calls() {

                // 1. PROPOSE decision
                let decision = self.propose_decision_from_tool(&tool_call, &state);

                // 2. VALIDATE (deterministico, < 50ms, sem LLM)
                let validation = self.governance.validate(&decision);
                self.audit.append(AuditEntry::validation(&validation));

                match validation.verdict {
                    Verdict::Deny => {
                        // BLOCKED — inject razoes e continua loop
                        let msg = format!("BLOCKED: {}", validation.reasons_text());
                        self.history.push(user_msg(&msg));
                        self.governance.record(decision.block(&validation));
                        state.record_blocked();
                        continue;
                    }
                    Verdict::Allow => {
                        // 3. APPROVE + ACTIVATE
                        let decision = decision.approve().activate();

                        // 4. EXECUTE tool
                        let result = self.registry.execute(&tool_call, repo).await;
                        state.record_tool_call(&tool_call, &result);

                        // 5. Decision outcome
                        match &result {
                            Ok(output) => {
                                self.governance.record(decision.complete(&output));
                                self.history.push(user_msg(&output.text));

                                // Check reuse hint
                                if let Some(reuse) = validation.reused_decision {
                                    self.history.push(user_msg(
                                        &format!("HINT: Decisao similar encontrada: {}. Abordagem pode ser reutilizada.", reuse)
                                    ));
                                }
                            }
                            Err(e) => {
                                self.governance.record(decision.fail(&e.to_string()));
                                self.history.push(user_msg(&format!("ERROR: {}", e)));
                            }
                        }
                    }
                }
            }

            // ── done() handling via PromiseGate ──
            // (DoneTool propoe Decision::Done → validate inclui PromiseGate check)
        }

        state.finalize()
    }
}
```

---

## Fluxo de uma tool call

```
1. LLM retorna tool_call (ex: edit_file)
2. Agent propoe Decision(type=Edit, scope={file}, payload={edit details})
3. GovernanceLayer.validate() → ValidationPipeline (< 50ms)
   - Scope ok? Time ok? Circuit breaker ok? Policies ok?
4a. DENY → inject "BLOCKED: {reasons}" no historico, continua loop
4b. ALLOW → aprova, ativa, executa tool
5. Resultado: complete() ou fail()
6. AuditLog.append() — registra tudo
```

---

## done() — Fluxo especial

Quando o LLM chama `done()`:
1. DoneTool propoe `Decision::Done`
2. ValidationPipeline inclui **PromiseGate** check
3. Se PromiseGate falha (ex: git diff vazio) → `PROMISE_UNMET` → DENY
4. Se passa → `Completed` → loop termina

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (phase) | ~15 | Transicoes validas/invalidas |
| Integration (mock LLM) | ~5 | Loop com governance + decisions |
| E2E | ~2 | Agent resolve bug + decision trace verificavel |
