# Fase 09 — Capabilities & Security

## Objetivo

Definir capability sets por agente que restringem quais tools podem ser usadas,
integrando com o sistema de permissões existente em `theo-domain`.

## Dependências

- Fase 01 (tipos base, ToolCategory)
- Fase 04 (ToolCallManager)

## Arquivos

### Novos

| Arquivo | Crate | Conteúdo | Linhas Est. |
|---------|-------|----------|-------------|
| `src/capability.rs` | theo-domain | `CapabilitySet`, `CapabilityDenied` | ~100 |
| `src/capability_gate.rs` | theo-agent-runtime | `CapabilityGate` | ~120 |

### Modificados

| Arquivo | Mudança |
|---------|---------|
| `theo-domain/src/lib.rs` | Adicionar `pub mod capability` |
| `theo-agent-runtime/src/tool_call_manager.rs` | Checar capabilities antes de dispatch |
| `theo-agent-runtime/src/lib.rs` | Adicionar `pub mod capability_gate` |

## Tipos Definidos

### theo-domain/src/capability.rs

```rust
pub struct CapabilitySet {
    pub allowed_tools: HashSet<String>,        // tool IDs; vazio = todos
    pub denied_tools: HashSet<String>,         // deny > allow
    pub allowed_categories: HashSet<ToolCategory>,
    pub max_file_size_bytes: u64,
    pub allowed_paths: Vec<String>,            // glob patterns
    pub network_access: bool,
}

impl CapabilitySet {
    pub fn unrestricted() -> Self;
    pub fn read_only() -> Self;
    pub fn can_use_tool(&self, tool_id: &str, category: ToolCategory) -> bool;
    pub fn can_write_path(&self, path: &str) -> bool;
}
```

### theo-agent-runtime/src/capability_gate.rs

```rust
pub struct CapabilityGate {
    capabilities: CapabilitySet,
    event_bus: Arc<EventBus>,
}

impl CapabilityGate {
    pub fn new(capabilities: CapabilitySet, event_bus: Arc<EventBus>) -> Self;

    pub fn check_tool(
        &self,
        tool_name: &str,
        tool_category: ToolCategory,
    ) -> Result<(), CapabilityDenied>;

    pub fn check_path_write(&self, path: &str) -> Result<(), CapabilityDenied>;
}

pub struct CapabilityDenied {
    pub tool_name: String,
    pub reason: String,
}
```

## Modelo de Capabilities

```rust
capabilities = {
    read_files,
    write_files,
    execute_shell,
    access_web,
    modify_state,
}
```

### Regras de Segurança

- Tools permitidas por allowlist
- Shell restrito por capability
- Escrita com escopo limitado por paths
- Confirmação para ações destrutivas
- Logs imutáveis de denied

### Proteções Obrigatórias

- Isolamento por sessão (já existente via SessionId)
- Sanitização de inputs externos (já existente via sandbox)
- Rate limiting (via Budget, Fase 07)

## Testes Requeridos (~10)

- `unrestricted()` permite tudo
- `read_only()` nega edit/write/bash
- Denied tools têm precedência sobre allowed categories
- Path patterns matched corretamente (glob)
- Tool call rejeitada quando capability check falha
- `can_use_tool` com allowed_tools vazio → permite tudo
- `can_use_tool` com denied_tools → nega especificamente
- `can_write_path` com paths permitidos
- `can_write_path` com paths não permitidos
- CapabilityDenied serde roundtrip

## Definition of Done

| # | Critério | Verificação |
|---|----------|-------------|
| 1 | `ToolCallManager::dispatch_and_execute` checa capabilities antes de executar | Code review |
| 2 | Tool calls negadas transicionam para `Failed` com razão de capability | Teste unitário |
| 3 | `CapabilitySet` integra com `ToolCategory` existente | Teste unitário |
| 4 | 10+ testes passando | `cargo test` |
