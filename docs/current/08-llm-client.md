# 08 — LLM Client

Abstrai a comunicacao com LLMs via trait, permitindo trocar backend sem modificar o agent loop.

---

## Estrutura

```
crates/agent/src/llm/
  mod.rs                   # LlmClient trait + tipos
  openai.rs                # Client OpenAI-compatible (vLLM, OpenAI, Anthropic)
  hermes.rs                # Parser fallback Hermes XML
  history.rs               # MessageHistory com compactacao
```

---

## LlmClient trait

```rust
#[async_trait]
trait LlmClient: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
}
```

### CompletionRequest / CompletionResponse

```rust
struct CompletionRequest {
    messages: Vec<ChatMessage>,
    tools: Vec<ToolDefinition>,
    temperature: f32,
    max_tokens: usize,
}

struct CompletionResponse {
    content: Option<String>,
    tool_calls: Vec<ToolCall>,
    usage: TokenUsage,
}

struct ToolCall {
    id: String,
    name: String,
    arguments: String,  // JSON
}
```

---

## OpenAI-compatible client

Unico client que suporta multiplos backends via URL base:

| Backend | URL base |
|---|---|
| vLLM (local) | `http://localhost:8000/v1` |
| OpenAI | `https://api.openai.com/v1` |
| Anthropic (via proxy) | Compativel com formato OpenAI |

Implementado com `reqwest` async.

---

## Hermes XML Parser

Fallback para modelos que usam formato Hermes (tool calls em XML dentro do content):

```xml
<tool_call>
{"name": "edit_file", "arguments": {"path": "...", "old_text": "...", "new_text": "..."}}
</tool_call>
```

O parser extrai tool calls do XML quando o modelo nao suporta tool calling nativo.

---

## MessageHistory com compactacao

```rust
struct MessageHistory {
    messages: Vec<ChatMessage>,
    max_tokens: usize,
}

impl MessageHistory {
    fn push(&mut self, msg: ChatMessage);

    /// Compacta historico quando excede max_tokens:
    /// - Mantem system message
    /// - Mantem ultimas N mensagens
    /// - Resume mensagens antigas
    fn compact(&mut self);

    fn as_messages(&self) -> &[ChatMessage];
}
```

---

## Testes esperados

| Tipo | Quantidade | O que testa |
|---|---|---|
| Unit (hermes) | ~10 | Parser XML — tool calls validos, malformed, edge cases |
| Unit (history) | ~5 | Compactacao, limites, mensagens preservadas |
| Integration (wiremock) | ~3 | Client HTTP com mock server |
