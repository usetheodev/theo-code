# theo-code-tools

Sistema de tools do theo-code -- um AI coding assistant open-source escrito em Rust.

Cada tool encapsula uma operacao atomica que o agente de IA pode invocar: ler arquivos, executar comandos, editar codigo, buscar na web, etc. O design e inspirado no [OpenCode](https://github.com/anomalyco/opencode), reescrito idiomaticamente em Rust com foco em seguranca, performance e testabilidade.

## Arquitetura

```
                    ToolRegistry
                         |
          +--------------+--------------+
          |              |              |
       BashTool      ReadTool      EditTool  ...  (22 tools)
          |              |              |
          +--------------+--------------+
                         |
                    Tool (trait)
                         |
              +----------+----------+
              |          |          |
         ToolContext  ToolOutput  PermissionCollector
              |
         theo-code-core
```

### Tool trait

Toda tool implementa o trait `Tool` definido em `theo-code-core`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError>;
}
```

- **`args`**: parametros da tool como JSON (validados internamente).
- **`ctx`**: contexto de execucao (session, project_dir, abort signal).
- **`permissions`**: coletor de permissoes -- cada tool registra as permissoes que precisa antes de executar. O caller decide se aprova ou nega.

### Pipeline de execucao

```
1. LLM envia tool_call com nome + parametros
2. ToolRegistry resolve a tool pelo id
3. Tool valida parametros (JSON -> tipos internos)
4. Tool registra permissoes necessarias (PermissionCollector)
5. Tool executa a operacao
6. Output passa por truncation automatico (2000 linhas / 50KB)
7. ToolOutput retorna ao caller com titulo, output, metadata e attachments
```

## Tools implementadas

### Operacoes de arquivo

| Tool | ID | Descricao | Testes |
|------|----|-----------|--------|
| **ReadTool** | `read` | Le arquivos com suporte a offset/limit, detecta binarios, retorna imagens como attachments | 18 |
| **WriteTool** | `write` | Escreve conteudo em arquivos, cria diretorios pai automaticamente | 11 |
| **EditTool** | `edit` | Substitui texto em arquivos, preserva line endings (LF/CRLF), suporta replaceAll | 16 |
| **ApplyPatchTool** | `apply_patch` | Aplica patches unificados com add/update/delete/move, suporta heredoc e EOF anchor | 13 |
| **MultiEditTool** | `multiedit` | Multiplas edicoes sequenciais em um unico arquivo | -- |

### Busca e navegacao

| Tool | ID | Descricao | Testes |
|------|----|-----------|--------|
| **GrepTool** | `grep` | Busca conteudo com regex via grep, limita a 100 matches | 6 |
| **GlobTool** | `glob` | Encontra arquivos por glob pattern, limita a 100 resultados | 2 |
| **LsTool** | `ls` | Lista conteudo de diretorio | -- |
| **LspTool** | `lsp` | Operacoes LSP: go-to-definition, references, hover (experimental) | -- |

### Execucao

| Tool | ID | Descricao | Testes |
|------|----|-----------|--------|
| **BashTool** | `bash` | Executa comandos shell com deteccao de permissoes, truncation automatico | 14 |

### Web e busca externa

| Tool | ID | Descricao | Testes |
|------|----|-----------|--------|
| **WebFetchTool** | `webfetch` | Busca URLs, retorna imagens como attachments, SVG como texto | 5 |
| **WebSearchTool** | `websearch` | Busca web via API externa | -- |
| **CodeSearchTool** | `codesearch` | Busca contexto de codigo via API externa | -- |

### Agentes e interacao

| Tool | ID | Descricao | Testes |
|------|----|-----------|--------|
| **TaskTool** | `task` | Spawna subagentes para tarefas especializadas | 1 |
| **SkillTool** | `skill` | Carrega skills especializadas com arquivos bundled | 2 |
| **QuestionTool** | `question` | Faz perguntas interativas ao usuario | 2 |

### Utilitarios

| Tool | ID | Descricao | Testes |
|------|----|-----------|--------|
| **TodoTool** | `todo` | Atualiza lista de tarefas da sessao | -- |
| **InvalidTool** | `invalid` | Placeholder de erro para tool calls invalidas | -- |
| **BatchTool** | `batch` | Executa ate 25 tool calls em paralelo (experimental) | -- |
| **PlanExitTool** | `plan_exit` | Sai do modo planejamento (experimental) | -- |

### Infraestrutura

| Modulo | Descricao | Testes |
|--------|-----------|--------|
| **registry** | Registro central de tools, discovery e criacao do registry padrao | 4 |
| **external_directory** | Validacao de permissao para acesso a caminhos fora do projeto | 5 |
| **truncate** | Truncation de output (re-export de theo-code-core) | 11 |

## Sistema de permissoes

Toda tool registra as permissoes necessarias **antes** de executar a operacao. O `PermissionCollector` acumula os requests e o caller (UI/CLI) decide se aprova.

```rust
permissions.record(PermissionRequest {
    permission: PermissionType::Bash,
    patterns: vec!["echo hello".to_string()],
    always: vec!["echo *".to_string()],
    metadata: serde_json::json!({}),
});
```

Tipos de permissao:

- `Read` -- leitura de arquivos (env files pedem permissao extra)
- `Edit` -- escrita/edicao de arquivos
- `Bash` -- execucao de comandos shell
- `Glob` / `Grep` -- busca no filesystem
- `WebFetch` -- acesso a URLs externas
- `Skill` -- carregamento de skills
- `Task` -- spawn de subagentes
- `ExternalDirectory` -- acesso a caminhos fora do diretorio do projeto

## Truncation

Output que excede os limites e truncado automaticamente:

- **2000 linhas** ou **50KB** (padrao)
- Direcao configuravel: head (padrao) ou tail
- Output completo salvo em arquivo quando truncado
- Mensagem de hint sugere usar Grep/Read/Task para acessar o output completo

## Testes

```bash
# Rodar todos os testes
cargo test --workspace

# Rodar apenas testes de tools
cargo test -p theo-code-tools

# Rodar testes de um modulo especifico
cargo test -p theo-code-tools bash
cargo test -p theo-code-tools edit
cargo test -p theo-code-tools apply_patch
```

### Cobertura por modulo

| Modulo | Testes | Cobertura |
|--------|--------|-----------|
| read | 18 | Permissoes, truncation, offset/limit, binarios, imagens, env files, linhas longas |
| edit | 16 | Criacao, edicao, replace_all, CRLF/LF preservation, diff stats, edge cases |
| bash | 14 | Execucao basica, permissoes, external_directory, truncation, cd-only, redirects |
| apply_patch | 13 | Add/update/delete, move, heredoc, EOF anchor, context disambiguation, validacao |
| truncate | 11 | Bytes, linhas, head/tail, file write, task hint |
| write | 11 | Criacao, overwrite, relative paths, JSON, empty, CRLF, readonly, title |
| grep | 6 | Busca basica, no matches, CRLF handling |
| webfetch | 5 | Image detection, SVG, content-type, base64 |
| external_directory | 5 | No-op, inside/outside project, directory kind, bypass |
| registry | 4 | Register, ids, default registry, empty |
| question | 2 | Execucao valida, header validation |
| skill | 2 | Sorting estavel, execute com files |
| glob | 2 | Matches, no matches |
| task | 1 | Sorting estavel de subagentes |
| **Total** | **119** | |

### Helpers de teste

- `TestDir` -- diretorio temporario com suporte a git init, write/read/exists
- `test_context()` -- cria `ToolContext` para testes
- `find_permission()` -- busca permissao por tipo no `PermissionCollector`

## Dependencias

| Crate | Uso |
|-------|-----|
| `tokio` | Runtime async |
| `async-trait` | Trait async para Tool |
| `serde` / `serde_json` | Serializacao de args e metadata |
| `thiserror` | Error types |
| `similar` | Diff computation (edit tool) |
| `regex` | Pattern matching (grep tool) |
| `glob` | File pattern matching (glob tool) |
| `reqwest` | HTTP client (webfetch tool) |
| `walkdir` | Directory traversal (skill tool) |
| `ignore` | Gitignore-aware traversal |
| `tempfile` | Diretorios temporarios (dev-dependency, testes) |

## Origem

Testes extraidos e reescritos a partir do [OpenCode](https://github.com/anomalyco/opencode) (TypeScript/Bun), preservando intencao, cobertura e comportamento esperado dos testes originais. A logica de producao foi reescrita idiomaticamente em Rust.
