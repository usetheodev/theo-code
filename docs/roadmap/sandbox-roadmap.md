# Roadmap: Sandbox de Execucao Segura

**ADR:** [002-sandbox-execution](../adr/002-sandbox-execution.md)
**Criado:** 2026-04-01
**Ultima atualizacao:** 2026-04-01

---

## Visao Geral

6 fases incrementais. Cada fase tem DoD (Definition of Done) e gate go/no-go.
Uma fase nao inicia sem o gate da anterior ser aprovado.

```
Fase 1 ──gate──► Fase 2 ──gate──► Fase 3 ──gate──► Fase 4 ──gate──► Fase 5 ──gate──► Fase 6
Domain          Filesystem       Process          Network          Governance       macOS+UI
types           isolation        isolation        isolation        integration
```

---

## Fase 1: Domain Types + ADR

**Objetivo:** Fundacao tipada. Definir contratos antes de implementar.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 1.1 | ADR-002 documentando decisao | `docs/adr/002-sandbox-execution.md` |
| 1.2 | Este roadmap | `docs/roadmap/sandbox-roadmap.md` |
| 1.3 | Tipos puros no domain | `crates/theo-domain/src/sandbox.rs` |
| 1.4 | Modulo registrado | `crates/theo-domain/src/lib.rs` |

### Tipos a implementar

```
SandboxConfig
├── enabled: bool
├── fail_if_unavailable: bool
├── filesystem: FilesystemPolicy
├── network: NetworkPolicy
├── process: ProcessPolicy
└── audit: AuditPolicy

FilesystemPolicy
├── allowed_read: Vec<String>    (path patterns)
├── allowed_write: Vec<String>
├── denied_read: Vec<String>     (sempre: ~/.ssh, ~/.gnupg, .env)
└── denied_write: Vec<String>    (sempre: /etc, /usr, /boot)

NetworkPolicy
├── allow_network: bool          (default: false)
├── allowed_domains: Vec<String>
├── denied_domains: Vec<String>
└── allow_dns: bool              (default: false)

ProcessPolicy
├── max_processes: u32           (default: 64)
├── max_memory_bytes: u64        (default: 512MB)
├── max_cpu_seconds: u64         (default: 120)
├── max_file_size_bytes: u64     (default: 100MB)
└── allowed_env_vars: Vec<String>

AuditPolicy
├── log_commands: bool
├── log_violations: bool
└── log_network: bool

ResourceLimits (resumo numerico de ProcessPolicy)

SandboxResult
├── success: bool
├── exit_code: i32
├── stdout: String
├── stderr: String
├── violations: Vec<SandboxViolation>
└── audit_entries: Vec<AuditEntry>
INVARIANTE: success=true implica violations vazio

SandboxViolation (enum)
├── FilesystemAccess { path, operation, denied_by }
├── NetworkAccess { address, port, denied_by }
├── ProcessLimit { resource, limit, attempted }
└── EnvironmentLeak { var_name }

AuditEntry
├── timestamp: String
├── event_type: String
├── detail: String
└── metadata: serde_json::Value
```

### Definition of Done

- [x] ADR-002 escrito e aprovado em meeting
- [x] Roadmap escrito com DoDs e gates
- [x] Todos os tipos implementados em `sandbox.rs`
- [x] Zero dependencias de OS no theo-domain
- [x] SandboxConfig::default() retorna valores seguros (fail_if_unavailable=true, network deny)
- [x] SandboxResult enforça invariante success/violations no construtor
- [x] Minimo 12 testes: serde round-trip, defaults, invariantes, edge cases (23 testes)
- [x] `cargo test -p theo-domain` passa sem falhas
- [x] Zero warnings novos no workspace

### Gate para Fase 2

- [x] Todos os itens do DoD acima
- [x] `/meeting` aprovada para Fase 2
- [x] Spike: confirmar que `landlock` crate funciona no kernel do ambiente de dev
- [x] Spike: confirmar viabilidade de user namespace sem root

---

## Fase 2: Filesystem Isolation (landlock)

**Objetivo:** Primeira camada real de isolamento. Controlar acesso a filesystem.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 2.1 | SandboxExecutor com landlock | `crates/theo-tooling/src/sandbox/executor.rs` |
| 2.2 | Kernel probe (feature detection) | `crates/theo-tooling/src/sandbox/probe.rs` |
| 2.3 | AST validation lexica | `crates/theo-tooling/src/sandbox/command_validator.rs` |
| 2.4 | Integracao com BashTool | `crates/theo-tooling/src/bash/mod.rs` |
| 2.5 | Denied list fixa | `crates/theo-tooling/src/sandbox/denied_paths.rs` |

### Mecanismos

- `landlock` crate para restricao de filesystem (Linux 5.13+)
- Kernel probe via `prctl` / `landlock_create_ruleset` na init
- AST validation: parse de comandos perigosos ANTES de execucao
- Denied list fixa: `~/.ssh`, `~/.gnupg`, `~/.config/gh`, `~/.aws`, `.env`, `.env.*`

### Dependencias novas

```toml
[workspace.dependencies]
landlock = "0.4"    # Filesystem isolation
```

### Definition of Done

- [x] SandboxExecutor funcional com landlock
- [x] Kernel probe detecta disponibilidade na init
- [x] Fail-closed: kernel < 5.13 → SandboxError::Unavailable → tool call rejeitada
- [x] Denied list fixa bloqueia acesso a ~/.ssh, ~/.gnupg, .env
- [x] AST validator rejeita patterns perigosos: `rm -rf /`, interpreter escape
- [x] BashTool usa SandboxExecutor quando sandbox habilitado
- [x] Testes com processos reais: confirmar que landlock bloqueia acesso
- [x] Teste de regressao: BashTool funciona identico com sandbox desabilitado
- [x] Structured logging de cada restricao aplicada
- [x] `cargo test -p theo-tooling` passa

### Gate para Fase 3

- [x] Todos os itens do DoD acima
- [x] Teste em ambiente real (nao so CI) confirmando landlock funcional
- [x] `/meeting` aprovada para Fase 3
- [x] Metricas: overhead medido (target: < 5ms por exec)

---

## Fase 3: Process Isolation + Resource Limits

**Objetivo:** Prevenir fork bombs, memory exhaustion, e token leaking.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 3.1 | setrlimit wrapper | `crates/theo-tooling/src/sandbox/rlimits.rs` |
| 3.2 | PID namespace (user ns) | `crates/theo-tooling/src/sandbox/namespaces.rs` |
| 3.3 | Env var sanitizer | `crates/theo-tooling/src/sandbox/env_sanitizer.rs` |
| 3.4 | Capability dropping | `crates/theo-tooling/src/sandbox/capabilities.rs` |
| 3.5 | Namespace reaper | `crates/theo-tooling/src/sandbox/reaper.rs` |

### Mecanismos

- `setrlimit(2)`: RLIMIT_CPU, RLIMIT_AS, RLIMIT_FSIZE, RLIMIT_NPROC
- `unshare(CLONE_NEWPID)` via user namespace
- Env var whitelist: PATH, HOME, USER, LANG, TERM. Tudo mais removido.
- `capset(2)`: drop CAP_NET_RAW, CAP_SYS_ADMIN, etc.
- Reaper goroutine: monitorar /proc/ns, cleanup periodicamente

### Dependencias novas

```toml
[workspace.dependencies]
nix = { version = "0.29", features = ["process", "resource", "sched"] }
```

### Definition of Done

- [x] setrlimit aplicado antes de cada exec
- [x] Fork bomb `:(){ :|:& };:` bloqueado por RLIMIT_NPROC (tested in child process)
- [x] File size limit enforced by RLIMIT_FSIZE (tested in child process)
- [ ] PID namespace isola processos filhos (deferred — PID ns adiado por complexidade)
- [x] Env vars sensiveis (AWS_*, GITHUB_TOKEN, etc) removidas
- [ ] Capabilities dropped apos fork, antes de exec (deferred — adiado)
- [ ] Namespace reaper ativo, sem leak em 1000+ execucoes (deferred — PID ns adiado)
- [ ] Testes de stress: 100 execs concorrentes sem leak (deferred)
- [x] Zero env var leak em subprocesso (teste com `env | grep`)

### Gate para Fase 4

- [x] Implementados: setrlimit + env sanitizer
- [ ] Stress test: 1000 execucoes sem namespace/fd leak (deferred — PID ns adiado)
- [x] `/meeting` aprovada para Fase 4
- [x] Spike: viabilidade de network namespace sem root (confirmado: unshare --user --net funciona)

---

## Fase 4: Network Isolation

**Objetivo:** Prevenir exfiltracao de dados via rede.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 4.1 | Network namespace | `crates/theo-tooling/src/sandbox/network.rs` |
| 4.2 | Domain whitelist | `crates/theo-tooling/src/sandbox/network_policy.rs` |
| 4.3 | DNS control | Integrado em network.rs |

### Mecanismos

- `unshare(CLONE_NEWNET)` via user namespace
- Default: rede completamente bloqueada no sandbox
- Whitelist de dominios para comandos que precisam de rede (ex: npm install)
- DNS queries bloqueadas por padrao (previne DNS exfil)

### Definition of Done

- [x] Network namespace aplicado por padrao (default deny)
- [x] `curl https://attacker.com` bloqueado dentro do sandbox
- [x] DNS exfil bloqueado (nslookup falha — net ns has no interfaces)
- [ ] Whitelist de dominios funcional para casos aprovados (deferred — requires proxy/nftables)
- [x] Teste de exfiltracao: confirmar que dados nao saem
- [ ] Teste de funcionalidade: npm install funciona com whitelist (deferred with whitelist)
- [x] Fallback documentado para envs sem user namespace

### Gate para Fase 5

- [x] Core DoDs atendidos (net ns + exfil blocked + fallback)
- [ ] Test matrix completo: Linux nativo, WSL2, Docker, Podman (partial — tested on Linux 6.8 native only)
- [x] `/meeting` aprovada para Fase 5

---

## Fase 5: Governance Integration

**Objetivo:** Sandbox como policy engine integrada a governance.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 5.1 | SandboxPolicyEngine | `crates/theo-governance/src/sandbox_policy.rs` |
| 5.2 | Sequence analyzer | `crates/theo-governance/src/sequence_analyzer.rs` |
| 5.3 | Audit trail | `crates/theo-governance/src/sandbox_audit.rs` |
| 5.4 | Configuracao TOML | Integrado em theo-application |

### Mecanismos

- Governance gera SandboxConfig dinamico por comando (mais restritivo para comandos arriscados)
- Sequence analysis: detecta toxic combinations (mkdir + chmod + exec)
- Audit trail: cada execucao sandboxed gera entrada auditavel
- TOML config: usuario configura defaults, admin pode lock

### Definition of Done

- [x] Governance gera SandboxConfig automaticamente por comando (sandbox_policy.rs)
- [x] Comandos mais arriscados recebem sandbox mais restritivo (CommandRisk enum: Low/Medium/High/Critical)
- [x] Sequence analyzer detecta toxic combinations (6 builtin patterns)
- [x] Audit trail completo de cada execucao (AuditTrail with thread-safe records)
- [ ] Config TOML funcional com defaults seguros (deferred — requires theo-application integration)
- [x] Testes: 26 testes em governance (sandbox_policy + sequence_analyzer + sandbox_audit)

### Gate para Fase 6

- [x] Core DoDs atendidos (policy engine + sequence analyzer + audit trail)
- [ ] Audit trail validado em sessao real de agente (requires runtime integration)
- [x] `/meeting` aprovada para Fase 6

---

## Fase 6: macOS Support + UI

**Objetivo:** Cross-platform e visibilidade para o usuario.

### Entregas

| # | Entrega | Arquivo |
|---|---|---|
| 6.1 | macOS sandbox backend | `crates/theo-tooling/src/sandbox/macos.rs` |
| 6.2 | Painel de status UI | `apps/theo-ui/src/components/sandbox/` |
| 6.3 | Policy lock corporativo | Integrado em config |

### Mecanismos

- macOS: avaliar `sandbox-exec` (deprecated) vs container-based (Lima/Colima)
- UI: painel mostrando restricoes ativas, violacoes recentes, status
- Policy lock: admin define config, usuario nao pode alterar

### Nota sobre macOS

`sandbox-exec` e deprecated desde macOS 10.15 e nao tem substituto oficial.
Opcoes realistas:
1. Container via Lima/Colima (overhead maior)
2. App Sandbox via entitlements (limitado para CLI)
3. Aceitar que macOS tera sandbox reduzido com warning explicito

### Definition of Done

- [x] macOS: stub backend com command validation + env sanitization (no kernel isolation)
- [ ] UI: painel de status implementado no desktop app (deferred — requires React frontend work)
- [x] Policy lock: PolicyLock struct implemented with field-level locking
- [ ] Testes em macOS real (deferred — requires macOS hardware)
- [x] Documentacao para usuario sobre limitacoes por plataforma (in macos.rs doc comments)

---

## Test Matrix (obrigatorio a partir da Fase 2)

| Ambiente | Landlock | User NS | Net NS | rlimits |
|---|---|---|---|---|
| Linux nativo 5.13+ | Sim | Sim | Sim | Sim |
| Linux nativo < 5.13 | Nao (fail-closed) | Sim | Sim | Sim |
| WSL2 | Verificar | Verificar | Verificar | Sim |
| Docker (rootless) | Verificar | Limitado | Limitado | Sim |
| Docker (root) | Sim | Sim | Sim | Sim |
| Podman (rootless) | Verificar | Sim | Limitado | Sim |
| macOS | Nao | Nao | Nao | Parcial |

---

## Dependencias por fase

| Fase | Crate externo | Motivo |
|---|---|---|
| 1 | Nenhum | Tipos puros |
| 2 | `landlock` | Filesystem isolation |
| 3 | `nix` | setrlimit, unshare, capset |
| 4 | `nix` (ja adicionado) | Network namespace |
| 5 | Nenhum novo | Governance integration |
| 6 | TBD | macOS-specific |

---

## Metricas de sucesso

| Metrica | Target | Medido em |
|---|---|---|
| Overhead por exec | < 5ms | Fase 2 |
| Fork bomb bloqueado | 100% | Fase 3 |
| Exfil bloqueada | 100% | Fase 4 |
| Namespace leak apos 1000 execs | 0 | Fase 3 |
| Env var leak | 0 | Fase 3 |
| False positive (comando legitimo bloqueado) | < 1% | Fase 5 |
