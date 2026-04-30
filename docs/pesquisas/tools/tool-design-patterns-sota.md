# Tool Design Patterns for AI Coding Agents -- SOTA Research

**Data:** 2026-04-29
**Dominio:** Tools
**Objetivo:** Elevar nota de 1.5/5 para 4.0+/5 no SCORECARD
**Pesquisador:** SOTA Validation Loop (deep-researcher)

---

## Sumario Executivo

O sistema de ferramentas e o componente de maior impacto pratico em um AI coding agent. Analise reversa do Claude Code (arXiv:2604.14228) mostra que 98.4% do codebase e infraestrutura deterministica -- e tools sao a maior fatia. Este documento consolida SOTA de 6 fontes primarias (OpenDev, Claude Code, Codex, Hermes, pi-mono, opencode), 4 benchmarks (tau-bench, BFCL, MCP-Atlas, SWE-bench), e 5 papers sobre qualidade de schema/descricao, cobrindo 10 topicos criticos para o design de ferramentas em Theo Code.

**Descobertas-chave:**

1. **Lazy tool discovery** reduz contexto de startup de 40% para <5% (OpenDev) -- Claude Code usa modelo identico com `ToolSearch` + deferred schemas
2. **Fuzzy edit matching** com 9 passes (OpenDev) elimina a maior fonte de erros de tools -- "content not found" em edits
3. **Tool result summarization** comprime 30K tokens para <100 tokens (300x), estendendo sessoes de 15-20 para 30-40 turnos
4. **Descricoes de qualidade** aumentam probabilidade de selecao correta em 260% (72% vs 20% baseline) -- paper com 10,831 MCP servers
5. **Parallelization** de read-only tools (ate 5 concorrentes) reduz latencia em 40-60% sem riscos de race condition
6. **Lifecycle hooks** (PreToolUse/PostToolUse) habilitam extensibilidade sem modificar handlers -- padrao adotado por Claude Code (12 eventos) e OpenDev (10 eventos)

---

## 1. Tool Design Patterns por Sistema

### 1.1 Comparacao Arquitetural

| Aspecto | OpenDev (Rust) | Claude Code (TS) | Codex (cloud) | Hermes (Python) | Theo Code (Rust) |
|---------|---------------|------------------|--------------|----------------|-----------------|
| **Total de tools** | 35+ | 54 (19 core + 35 conditional) | Sandbox-based | 58+ | 72 (manifest) |
| **Trait/interface base** | `BaseTool` async trait | Class per tool + `isEnabled()` | AGENTS.md + MCP | `handle_function_call()` | `ToolDef` trait |
| **Registry** | `ToolRegistry` (RwLock HashMap) | `getAllBaseTools()` + filtering | MCP + Skills | Registry central | `create_default_registry()` |
| **Schema source** | 3 (built-in + MCP + subagent) | Built-in + MCP + Skills | MCP | Built-in | Built-in + MCP |
| **Categorias** | 12 enum variants | Mode-based filtering | Sandbox permissions | Parallel-safe classification | ToolExposure enum |
| **Lazy loading** | `ToolSearch` + `should_defer()` | `ToolSearch` + deferred list | N/A (sandbox) | Nao | Nao (gap) |
| **Fuzzy edit** | 9-pass chain | Exact match only | Diff-based | Nao mencionado | Exact match (gap) |
| **Result truncation** | Per-tool rules + overflow files | Per-tool truncation | Sandbox-contained | Nao mencionado | `truncate/mod.rs` |
| **Parallelization** | `ParallelPolicy` trait-based | Parallel read-only | Multi-thread sandbox | Parallel-safe classification | Nao (gap) |
| **Lifecycle hooks** | 10 events | 12+ events | Automations | Before/after decorators | Nao (gap) |

### 1.2 OpenDev: BaseTool Trait (Referencia Primaria)

O `BaseTool` trait do OpenDev (arquivo: `crates/opendev-tools-core/src/traits.rs`) e a implementacao SOTA mais completa em Rust. 18 metodos no trait:

**Identidade:**
- `name()`, `description()`, `parameter_schema()` -- identidade basica
- `search_hint()` -- frase curta para keyword matching no ToolSearch
- `display_meta()` -- metadata para TUI (verb, label, category, primary_arg_keys)

**Classificacao (substitui listas hardcoded):**
- `is_read_only(args)` -- input-dependent (Bash com `ls` = read-only, Bash com `rm` = nao)
- `is_destructive(args)` -- mais estrito que `!is_read_only()`
- `is_concurrent_safe(args)` -- default delega para `is_read_only()`
- `category()` -- enum `ToolCategory` com 12 variantes (Read, Write, Process, Web, Session, Memory, Meta, Messaging, Automation, Symbol, Mcp, Other)
- `skip_dedup()` -- tools como Agent e SendMessage desabilitam dedup

**Lifecycle:**
- `is_enabled()` -- check runtime (feature flags, environment)
- `interrupt_behavior()` -- Cancel/Block/Ignore

**Result handling:**
- `truncation_rule()` -- per-tool override do sanitizer default
- `prompt_contribution()` -- texto que o tool contribui ao system prompt
- `should_defer()` -- se deve ser lazy-loaded via ToolSearch
- `format_validation_error(errors)` -- erro LLM-friendly customizado

**Execucao:**
- `execute(args, ctx)` -- async execution com `ToolContext` (working_dir, is_subagent, session_id, cancel_token, diagnostic_provider, shared_state)

### 1.3 Claude Code: 54 Tools com Filtragem Condicional

Segundo Liu et al. (arXiv:2604.14228), Claude Code v2.1.88:

- `getAllBaseTools()` retorna array de ate 54 tools
- 19 always-included: BashTool, FileReadTool, AgentTool, SkillTool, etc.
- 35 condicionalmente incluidos: feature flags, env vars, user type
- Modo `CLAUDE_CODE_SIMPLE`: apenas Bash, Read, Edit
- Cada tool tem `isEnabled()` para check runtime
- **1.6% do codebase e logica AI; 98.4% e infraestrutura deterministica**

**Lifecycle hooks (12+ eventos):**

| Evento | Tipo | Descricao |
|--------|------|-----------|
| `PreToolUse` | Blocking | Antes de qualquer tool -- pode aprovar/negar/modificar |
| `PostToolUse` | Informational | Apos tool -- log, linters, formatacao |
| `PostToolUseFailure` | Informational | Apos falha de tool |
| `Stop` | Blocking | Quando Claude termina resposta |
| `SubagentStop` | Blocking | Quando subagent completa |
| `PermissionRequest` | Blocking | Auto-approve ou deny |
| `PreCompact` | Blocking | Antes de compaction -- backup transcripts |
| `SessionStart` | Informational | Inicio de sessao |
| `SessionEnd` | Informational | Fim de sessao |
| `UserPromptSubmit` | Informational | Submissao de prompt |
| `Notification` | Blocking | Roteamento para Slack, TTS |
| `Setup` | Blocking | Via --init, --init-only, --maintenance |
| `Elicitation` | Blocking | Intercepta MCP server elicitation (v2.1.76+) |

### 1.4 Hermes: 58+ Tools com Parallel-Safe Classification

Hermes-agent classifica tools automaticamente como parallel-safe. O routing central `handle_function_call()` despacha por nome. Terminal backends multiplos (Local, Docker, SSH, Daytona, Modal, Singularity). Dangerous command detection + approval workflow.

### 1.5 Codex: Sandbox-First Design

Codex opera em containers isolados com internet desabilitada. Skills bundleiam instrucoes + recursos + scripts. AGENTS.md guia comportamento. Multi-thread com worktrees para evitar conflitos. Automatic Reviewer Agent para prompts elegiveis.

---

## 2. Tool Schema Quality

### 2.1 Impacto Quantificado de Descricoes de Qualidade

Dois papers de Fevereiro 2026 quantificaram o impacto de descricoes de tools em MCP:

**Paper 1: Hasan et al. (arXiv:2602.14878) -- 856 tools, 103 MCP servers:**

| Componente da descricao | Impacto no task success | Impacto no step count |
|--------------------------|------------------------|-----------------------|
| Todas as 6 componentes | +5.85pp mediana | +67.46% passos |
| Componentes compactas selecionadas | Preserva reliability | Reduz overhead |
| Sem augmentacao (baseline) | Baseline | Baseline |
| Regressao em 16.67% dos casos | -performance | -- |

**6 componentes identificadas:** Purpose, Parameters, Return Value, Examples, Constraints, Side Effects.

**Paper 2: arXiv:2602.18914 -- 10,831 MCP servers, 18 smell categories:**

| Dimensao de qualidade | Impacto na selecao de tool | p-value |
|------------------------|---------------------------|---------|
| Functionality | +11.6% | p < 0.001 |
| Accuracy | +8.8% | p < 0.001 |
| Standard-compliant description | 72% selection probability | -- |
| Baseline (smell-heavy) | 20% selection probability | -- |
| Nome repetido (prevalencia) | 73% dos servers | -- |

**Conclusao:** Descricoes compliant com standards atingem **260% mais probabilidade de selecao correta** (72% vs 20% baseline).

### 2.2 Best Practices para JSON Schema de Tools

Consolidando OpenDev, Claude Code, e os papers:

| Pratica | Evidencia | Threshold |
|---------|-----------|-----------|
| Description com Purpose + Parameters + Examples | +5.85pp task success | Obrigatorio |
| < 20 tools no contexto por turno | Recomendacao OpenAI | Usar lazy loading acima de 20 |
| Enum constraints em parametros | Reduz hallucination de args | Sempre que aplicavel |
| `required` field explicito | Evita args missing | Obrigatorio |
| Nomes sem repeticao do tool name | 73% violam (arXiv:2602.18914) | Auditar |
| Few-shot examples inline | +5.85pp quando compactos | 1-2 exemplos por tool |
| `search_hint` para ToolSearch | OpenDev BaseTool trait | Para tools deferidos |

### 2.3 Anti-Patterns de Schema

| Anti-Pattern | Prevalencia | Impacto |
|-------------|-------------|---------|
| Descricao vazia ou so nome | 73% MCP servers | -52pp selection |
| Descricao > 500 tokens | ~15% MCP servers | +67% steps, -reliability |
| Parametros sem type constraint | Comum | Hallucination de args |
| Descricao com jargao interno | -- | LLM nao entende |
| Duplicacao de semantica entre tools | -- | Confusao na selecao |

---

## 3. Fuzzy Edit Matching

### 3.1 O Problema

O edit tool e a ferramenta mais fragil em qualquer coding agent. O LLM deve fornecer `old_string` que corresponda **exatamente** ao conteudo do arquivo. Na pratica:

- Whitespace extra/faltando (tabs vs spaces, trailing spaces)
- Indentacao incorreta (comum em contexto truncado)
- Caracteres escapados incorretamente (aspas, backslash)
- Conteudo mudou desde o ultimo `read` (stale read)
- Duplicatas no arquivo (match ambiguo)

Erro tipico: `"String to replace not found"` -- mega-thread com 27+ issues no GitHub.

### 3.2 OpenDev: 9-Pass Chain-of-Responsibility

OpenDev implementa uma cadeia de responsabilidade com 9 passes ordenados por rigor decrescente. **Short-circuits no primeiro match** -- zero overhead para exact matches:

| Pass | Estrategia | O que relaxa | Overhead |
|------|-----------|--------------|----------|
| 1 | Exact match | Nada | Zero |
| 2 | Whitespace normalization | Trailing whitespace | Minimo |
| 3 | Indentation normalization | Leading whitespace levels | Minimo |
| 4 | Escape normalization | Quotes, backslashes | Minimo |
| 5 | Line-ending normalization | CRLF vs LF | Minimo |
| 6 | Anchor matching | Usa primeira/ultima linha como ancora | Medio |
| 7 | Prefix/suffix matching | Corta linhas extras | Medio |
| 8 | Fuzzy similarity (Levenshtein) | Threshold de similaridade | Alto |
| 9 | LLM-assisted repair | Fallback para LLM | Muito alto |

**Design decisions:**
- Pass 1-5: transformacoes deterministicas, sem falsos positivos
- Pass 6-7: heuristicas de ancora, pode ter ambiguidade
- Pass 8-9: ultimo recurso, caro e potencialmente impreciso

### 3.3 Abordagens Alternativas

| Sistema | Abordagem | Passes | Threshold |
|---------|----------|--------|-----------|
| OpenDev | Chain-of-responsibility 9 passes | 9 | Short-circuit |
| Gemini CLI | Exact -> Flexible -> Regex -> LLM repair | 4 | -- |
| RooCode | Middle-out fuzzy (Levenshtein + start_line hint) | 2-3 | 0.8-1.0 |
| pi-coding-agent | `fuzzyFindText` (exact -> normalized) | 2 | -- |
| Kilo Code | Overlapping window + Levenshtein | 2-3 | 0.8-1.0 |
| Qwen Code | Self-correction via model refinement | 1-2 | -- |
| Morph | Semantic/AST-level matching | 1 | Syntax tree |
| Hashline | Content-addressable line hashing | 0 (deterministic) | -- |

### 3.4 Stale-Read Detection

OpenDev implementa `FileTimeTracker` que detecta quando o arquivo foi modificado desde o ultimo read:
- Tolerancia: 50ms
- Se stale: injeta warning no ToolResult com `llm_suffix`
- Forca re-read antes de edit

### 3.5 Threshold para Theo Code

| Metrica | Minimo SOTA | Ideal | Theo Code Atual |
|---------|-------------|-------|-----------------|
| Passes de fallback | 3 (exact + whitespace + indent) | 5-9 | 1 (exact only) |
| Stale-read detection | Sim | Sim, <50ms | Nao |
| Indentation preservation | Sim | Sim | Sim |
| Unique match enforcement | Sim | Sim | Sim |
| Error message quality | LLM-friendly com diff context | + suggestion | Basico |

---

## 4. Tool Result Optimization

### 4.1 Per-Tool-Type Summarization (OpenDev)

OpenDev's `tool_summarizer.rs` cria resumos de 50-200 chars por tipo de tool:

| Tipo de tool | Resumo gerado | Tokens economizados |
|-------------|---------------|---------------------|
| File read | `"Read file (142 lines, 4831 chars)"` | 95%+ |
| Search | `"Search completed (23 matches found)"` | 90%+ |
| Directory listing | `"Listed directory (47 items)"` | 90%+ |
| Command (short) | Verbatim (< 100 chars) | 0% |
| Command (long) | `"Command executed (312 lines of output)"` | 95%+ |
| Error | Truncated a 200 chars com prefixo classificado | 80%+ |
| Test suite | 30,000 tokens -> < 100 tokens | **99.7%** |

**Impacto medido:**
- Sessoes estendidas de 15-20 turnos para 30-40 turnos sem compaction
- Reducao de 70-80% do consumo de contexto por tool outputs
- Single test suite: 30,000 -> < 100 tokens (compressao 300x)

### 4.2 Large Output Offloading (Overflow Files)

Quando output excede threshold (default 8,000 chars no OpenDev):

1. Output completo salvo em `~/.opendev/scratch/<session_id>/tool_<timestamp>_<name>.txt`
2. Contexto recebe: preview de 500 chars + path de referencia
3. Hint contextual: se agent tem subagent -> "Delegate to Code Explorer"; se e subagent -> "Use search tool with offset/limit"
4. Overflow files retidos por 7 dias, max 1MB cada
5. Cleanup automatico ao startup

**OpenDev `ToolResultSanitizer` -- truncation rules:**

| Tool | Max chars | Strategy |
|------|-----------|----------|
| Bash | 8,000 | Tail (output recente) |
| Read | 15,000 | Head |
| Grep | 10,000 | Head |
| Glob | 10,000 | Head |
| WebFetch | 12,000 | Head |
| WebSearch | 10,000 | Head |
| Browser | 5,000 | Head |
| Session history | 15,000 | Tail |
| Memory search | 10,000 | Head |
| MCP tools (default) | 8,000 | Head |
| Errors (all) | 2,000 | Head |

**Truncation strategies:**
- `Head`: manter inicio (bom para search results, file content)
- `Tail`: manter final (bom para command output, logs)
- `HeadTail`: manter inicio e final, cortar meio (bom para outputs grandes com header + resultado)

### 4.3 Theo Code Status

`theo-tooling/src/truncate/mod.rs` existe mas precisa ser auditado para:
- Cobertura de todos os 72 tools
- Strategies per-tool-type (Head/Tail/HeadTail)
- Overflow file storage
- Summarization layer (ausente)

---

## 5. MCP Lazy Discovery

### 5.1 O Problema do Contexto Eager

Loading de todos os schemas MCP no startup consome 40% do contexto disponivel (OpenDev, medido). Com 97 milhoes de downloads mensais do MCP SDK (Abril 2026), o numero de tools MCP disponiveis cresce rapidamente. Carregar 50+ tools no contexto:

- **Custo**: cada tool definition consome ~200-500 tokens
- **Latencia**: mais tokens de input = resposta mais lenta
- **Ruido**: modelos degradam quando contexto esta cheio de opcoes irrelevantes
- **Recomendacao OpenAI**: < 20 functions por turno

### 5.2 Padrao: Lazy Discovery via ToolSearch

Implementado por OpenDev e Claude Code com design identico:

```
Startup:
  1. Core tools (15-20) -> schemas completos no system prompt
  2. Deferred tools (15-35) -> apenas nome + descricao curta em <available-deferred-tools>
  3. ToolSearch tool -> sempre core, permite buscar schemas

Runtime:
  1. LLM precisa de tool nao-core
  2. Chama ToolSearch("select:WebFetch,WebSearch")
  3. Registry retorna schemas completos
  4. Schemas ativados para API calls subsequentes
```

**OpenDev ToolSearch (arquivo: `opendev-tools-impl/src/agents/tool_search.rs`):**
- 3 modos de query: `select:Name1,Name2`, keyword search, `+prefix terms`
- Scoring: name match = 3 pontos, description match = 1 ponto
- Retorna schemas completos em JSON + metadata de `activated_tools`
- `max_results` default: 5

**OpenDev Registry suporte:**
- `core_tools: RwLock<HashSet<String>>` -- tools sempre incluidos
- `mark_as_core()`, `mark_core_tools()` -- marcacao de core tools
- `get_schemas_for(names)` -- schemas apenas para tools especificos
- `get_deferred_summaries()` -- `(name, description)` dos nao-core
- `has_deferred_tools()` -- verifica se deferral esta ativo
- `BaseTool::should_defer()` -- trait method para auto-classificacao

### 5.3 MCP 2026 Roadmap

Evolucoes relevantes:
- **SEP-1649 (Server Cards)**: metadata padronizada sobre capacidades
- **SEP-1960 (.well-known manifest)**: discovery sem conexao ativa
- **MCP Registry**: "app store" centralizado para MCP servers
- **Transport evolution**: servers sem estado para scaling horizontal

### 5.4 Impacto Medido

| Metrica | Eager loading | Lazy discovery | Reducao |
|---------|--------------|----------------|---------|
| Contexto no startup | 40% | <5% | 87.5% |
| Tools disponiveis | Todos | Core + on-demand | Igual funcionalidade |
| First-turn latency | Alta | Baixa | Significativa |
| Sessoes antes de compaction | 15-20 turnos | 30-40 turnos | 2x |

### 5.5 Theo Code Status

`theo-infra-mcp/src/discovery.rs` existe com discovery basico. **Gaps:**
- Sem `ToolSearch` meta-tool
- Sem conceito de core vs deferred tools
- Sem `should_defer()` no trait
- Sem scoring/ranking de resultados de discovery

---

## 6. Tool Parallelization

### 6.1 Padrao SOTA: Read-Only Parallel, Write Sequential

Todos os sistemas SOTA convergem para o mesmo padrao:

| Classificacao | Execucao | Exemplos |
|---------------|----------|----------|
| Read-only | Paralelo (ate 5 concorrentes) | Read, Grep, Glob, WebSearch, WebFetch |
| Write (nao-destrutivo) | Sequencial | Edit, Write |
| Destructivo | Sequencial + approval | Bash com `rm`, `git push --force` |

### 6.2 OpenDev ParallelPolicy

`crates/opendev-tools-core/src/parallel.rs`:

**2 modos de particionamento:**
1. **`partition_with_tools()` (preferido)**: consulta `BaseTool::is_concurrent_safe(args)` -- decisao input-dependent
2. **`partition()` (deprecated)**: lista hardcoded de read-only tools

**Algoritmo:**
```
Input: [Read, Grep, Edit, Read, Glob]
Groups: [[0,1], [2], [3,4]]
         ^^^^   ^^^  ^^^^^^
         parallel serial parallel
```

- Tools consecutivos concurrent-safe -> batch unico (paralelo)
- Tool nao-concurrent -> batch proprio (serial)
- Batches executam em ordem posicional estrita (preserva intencao do LLM)

**Inovacao do OpenDev**: `is_read_only(args)` e input-dependent. Bash com `ls` e read-only; Bash com `rm` nao e. Isso permite parallelizar Bash reads com outros read-only tools, impossivel com classificacao estatica.

### 6.3 Claude Code Parallelization

- Read-only tools executam em paralelo
- Multiple `spawn_subagent` calls no mesmo response -> `asyncio.gather()` -> concurrent
- Cada subagent tem iteration budget e tool worker pool proprios

### 6.4 Hermes Parallel-Safe Classification

Classificacao automatica de tools como parallel-safe. Mecanismo exato nao documentado publicamente, mas o pattern e identico: read-only = parallel, write = sequential.

### 6.5 Threshold para Theo Code

| Metrica | Minimo SOTA | Ideal | Theo Code Atual |
|---------|-------------|-------|-----------------|
| Parallel read-only | Sim | Sim, input-dependent | Nao |
| Max concurrent tools | 5 | 5-10 configurable | 1 |
| Sequential writes | Enforced | Enforced | N/A (tudo serial) |
| Batch partitioning | Static list | Trait-based, input-dependent | Nao |

---

## 7. Benchmarks de Tool-Use

### 7.1 tau-Bench (Sierra Research)

**Paper:** Yao et al., arXiv:2406.12045 (2024), evolucao: tau2-bench (2025), tau3-bench (2026)

| Aspecto | Detalhes |
|---------|---------|
| **O que mede** | Interacao tool-agent-user em dominios reais (airline, retail, banking, telecom) |
| **Metodologia** | User simulado por LLM + agent com API tools + policy guidelines |
| **Metrica chave** | `pass^k` -- reliability over k trials (nao so accuracy unica) |
| **Dominios** | Airline, Retail (original); Banking, Telecom (tau2/tau3); Voice modality (tau3) |
| **Resultado SOTA** | GPT-4o < 50% tasks; pass^8 < 25% em retail |
| **Claude** | Claude 3.7 top performer; pass^k metric adotada por Anthropic internamente |
| **Evolucao** | tau2 introduz dual-control (Dec-POMDP); tau3 adiciona banking + voice |
| **Fixes** | 75+ correcoes aplicadas (SABER, Cuadron et al. 2025) |

**Relevancia para Theo Code:** tau-bench mede exatamente o que um coding agent faz -- multi-turn tool use com policies. A metrica `pass^k` e mais relevante que accuracy unica porque mede **consistencia**.

### 7.2 Berkeley Function Calling Leaderboard (BFCL)

**Paper:** Patil et al., ICML 2025 (PMLR 267)

| Aspecto | Detalhes |
|---------|---------|
| **Versao atual** | BFCL V4 |
| **O que mede** | Function calling accuracy em serial/parallel, multi-linguagem (Python, Java, JS, REST) |
| **Metodologia** | AST evaluation metric (escala para milhares de funcoes) |
| **Categorias V4** | Simple, Parallel, Multiple, Relevance Detection, Multi-turn, Web Search, Memory |
| **Dataset** | 2K question-function-answer pairs |
| **SOTA rankings (2026)** | Claude Opus 4.1: 70.36% (#2), Claude Sonnet 4: 70.29% (#3), GPT-5: 59.22% (#7) |
| **Achado-chave** | Top models excelentes em single-turn, mas falham em memory + long-horizon |
| **Status** | De facto standard para function calling evaluation |

**Relevancia para Theo Code:** BFCL mede qualidade de schema design (o quanto o modelo acerta tool calls). Schemas mal projetados resultam em scores baixos. Relevance Detection mede se o modelo sabe quando **nao** chamar uma tool.

### 7.3 MCP-Atlas

**Paper:** arXiv:2602.00933

Benchmark large-scale com MCP servers reais. Mede competencia de tool-use em cenarios realistas com servers MCP.

### 7.4 Implicacoes para SOTA Validation

| Benchmark | O que valida | Como usar no Theo Code |
|-----------|-------------|------------------------|
| tau-bench | Multi-turn tool reliability | Medir pass^k do agent em cenarios de coding |
| BFCL | Schema quality + selection accuracy | Validar schemas dos 72 tools contra BFCL categories |
| MCP-Atlas | MCP server interop | Testar theo-infra-mcp contra servers reais |
| SWE-bench | End-to-end coding tasks | Benchmark geral do agent |

---

## 8. Registry Architecture

### 8.1 ToolSchemaBuilder: 3 Sources

Pattern encontrado em OpenDev e Claude Code:

```
ToolSchemaBuilder
  |
  +-- Built-in tools (BaseTool implementations)
  |     - 19-35 tools hardcoded no binario
  |     - Schema definido em parameter_schema()
  |
  +-- MCP discovered tools
  |     - tools/list via MCP protocol
  |     - Schema vem do MCP server
  |     - Prefixo: mcp__<server>__<tool>
  |
  +-- Subagent tools
        - spawn_subagent, delegate_to_agent
        - Schema gerado dinamicamente
        - Tipos de subagent definem allowed_tools
```

### 8.2 ToolRegistry: Dispatch Pattern

OpenDev `ToolRegistry` (`opendev-tools-core/src/registry/mod.rs`):

| Feature | Implementacao |
|---------|-------------|
| Storage | `RwLock<HashMap<String, Arc<dyn BaseTool>>>` |
| Lookup | `get(name)` com alias fallback |
| Aliases | Legacy name -> canonical name mapping |
| Middleware | `Vec<Arc<dyn ToolMiddleware>>` pipeline |
| Dedup | `Mutex<HashMap<String, ToolResult>>` per-turn cache |
| Timeouts | Per-tool override map |
| Sanitizer | `ToolResultSanitizer` integrado |
| Overflow | Optional directory para outputs grandes |
| Core tools | `HashSet<String>` para lazy loading |
| Category map | `build_category_map()` de trait methods |

### 8.3 Handler Categories (OpenDev)

12 categorias definidas como enum `ToolCategory`:

| Categoria | Tools tipicos | Policy |
|-----------|--------------|--------|
| Read | Read, Glob, Grep, find_symbol | Parallel-safe, no approval |
| Write | Edit, Write | Sequential, soft approval |
| Process | Bash | Input-dependent, pattern-based approval |
| Web | WebFetch, WebSearch, screenshot | Parallel-safe, read-only |
| Session | list_sessions, spawn_subagent | Mixed |
| Memory | memory_search, memory_write | Mixed |
| Meta | TaskList, TaskUpdate, ToolSearch | Parallel-safe |
| Messaging | SendMessage | No dedup, sequential |
| Automation | Scheduling, cron | Sequential, approval |
| Symbol | find_symbol, find_referencing_symbols | Parallel-safe |
| Mcp | mcp__* bridge tools | Per-server rules |
| Other | Default fallback | Conservative |

### 8.4 Theo Code Registry Status

`theo-tooling/src/registry.rs` existe com `create_default_registry()`. `tool_manifest.rs` enumera 72 tools com `ToolExposure` (DefaultRegistry, MetaTool, ExperimentalModule, InternalModule) e `ToolStatus` (Implemented, Partial, Stub).

**Gaps em relacao ao SOTA:**
- Sem middleware pipeline
- Sem dedup cache per-turn
- Sem per-tool timeout overrides
- Sem category-based policy dispatch
- Sem overflow file storage integrada
- Sem core vs deferred tool distinction

---

## 9. Lifecycle Hooks

### 9.1 Pattern SOTA

Lifecycle hooks permitem extensibilidade sem modificar handlers de tools. Pattern adotado por:

| Sistema | Eventos | Tipo | Mecanismo |
|---------|---------|------|-----------|
| Claude Code | 12+ | Script/prompt/subagent handler | settings.json config |
| OpenDev | 10 | JSON stdin protocol, exit code 2 = block | External scripts |
| pi-mono | ~6 | beforeToolCall/afterToolCall | Inline hooks |
| Hermes | ~4 | Decorators | Python decorators |
| rippletide | ~5 | Rule-based governance | Hook injection |

### 9.2 OpenDev Lifecycle Events

| Evento | Quando | Pode bloquear? |
|--------|--------|----------------|
| SESSION_START | Inicio de sessao | Nao |
| USER_PROMPT_SUBMIT | Usuario envia prompt | Nao |
| PRE_TOOL_USE | Antes de executar tool | Sim (exit code 2) |
| POST_TOOL_USE | Apos tool executar | Nao |
| POST_TOOL_USE_FAILURE | Apos tool falhar | Nao |
| SUBAGENT_START | Subagent criado | Nao |
| SUBAGENT_STOP | Subagent termina | Nao |
| PRE_COMPACT | Antes de compaction | Nao |
| SESSION_END | Fim de sessao | Nao |
| STOP | Agent para | Nao |

**Mecanismo:** External scripts recebem JSON via stdin com evento + dados. Exit code 0 = allow, exit code 2 = block, qualquer outro = allow com warning.

### 9.3 Claude Code Hooks: 3 Handler Types

1. **Command handler**: executa shell command, recebe dados via stdin
2. **Prompt handler**: envia prompt para LLM, resultado guia acao
3. **Subagent handler**: delega para subagent especializado

**Matcher Groups**: regex filter que determina quais tool uses disparam o hook. Exemplo: `tool_name: "^Bash$"` dispara apenas para Bash.

### 9.4 Casos de Uso de Hooks

| Caso de Uso | Hook | Handler |
|-------------|------|---------|
| Auto-format apos edit | PostToolUse (matcher: Edit) | `prettier --write $file` |
| Lint apos write | PostToolUse (matcher: Write) | `eslint $file` |
| Block rm -rf | PreToolUse (matcher: Bash) | Script que analisa comando |
| Log de auditoria | PostToolUse (all) | Script que append JSONL |
| Backup antes de compaction | PreCompact | Script que salva transcript |
| Notify Slack on completion | Stop | Script que posta no Slack |

### 9.5 Theo Code Status

**Nao implementado.** Maior gap em relacao ao SOTA. Hooks habilitam:
- Extensibilidade sem fork
- Governance rules (rippletide pattern)
- Auto-formatting
- Audit logging
- Custom approval workflows

---

## 10. Design Evolution Lessons

### 10.1 De Flat Namespace para Category-Based Handlers

**Evolucao observada em OpenDev:**

| Fase | Design | Problema |
|------|--------|----------|
| 1. Flat | `HashMap<String, Tool>` | Sem agrupamento para policy |
| 2. Static groups | `tool_groups()` hardcoded | Brittle, esqueciam novos tools |
| 3. Category enum | `ToolCategory` com 12 variantes | Compile-time exhaustive matching |
| 4. Trait-based | `BaseTool::category()` | Auto-discovery, sem lista manual |

**Licao:** Mover classificacao para o trait forca compile-time completeness -- adicionar tool sem definir categoria causa warning/erro.

### 10.2 De Eager Loading para Lazy Discovery

| Fase | Design | Context overhead |
|------|--------|-----------------|
| 1. All schemas | Todos os tools no system prompt | 40% |
| 2. Mode filtering | Plan mode = read-only tools | 20-40% |
| 3. Lazy discovery | Core + ToolSearch on-demand | <5% |

**Licao:** "Bound every resource that grows with session length" (OpenDev paper). MCP lazy discovery e o padrao mais impactante para context engineering.

### 10.3 De Exact Match para Fuzzy Chain

| Fase | Design | Edit failure rate |
|------|--------|-------------------|
| 1. Exact only | `str.contains(old_string)` | Alta (~15-20%) |
| 2. Normalized | Whitespace/escape normalization | Media (~5-10%) |
| 3. Multi-pass | 9-pass chain-of-responsibility | Baixa (~1-3%) |

**Licao:** "Design tools to absorb LLM imprecision" (OpenDev paper). O modelo nunca sera perfeito em reproduzir whitespace exato.

### 10.4 De Outputs Completos para Summarization + Overflow

| Fase | Design | Context usage |
|------|--------|---------------|
| 1. Full output | Tool output direto no contexto | 70-80% do contexto |
| 2. Truncation | Corta apos N chars | 30-40% do contexto |
| 3. Summary + overflow | Resumo inline + full em arquivo | <20% do contexto |

**Licao:** Tool outputs sao a maior fonte de consumo de contexto (70-80%). Summarization e o maior ROI em context engineering.

### 10.5 De Permission Checks para Schema Gating

| Fase | Design | Seguranca |
|------|--------|-----------|
| 1. Runtime blocks | Tool tenta executar, permissao negada | LLM tenta de novo |
| 2. Schema gating | Tool invisivel para o LLM | LLM nao pode tentar |

**Licao:** "Make unsafe tools invisible, not blocked" (OpenDev). Remover do schema > bloquear no runtime.

---

## 11. Thresholds para o SOTA Validation Loop

| Metrica | Threshold Minimo | Threshold Ideal | Source | Theo Code Atual | Gap |
|---------|-----------------|-----------------|--------|-----------------|-----|
| Tool trait methods | 8 (name, desc, schema, execute, is_read_only, category, is_enabled, truncation_rule) | 15+ (full OpenDev) | OpenDev BaseTool | ~4 | Alto |
| Fuzzy edit passes | 3 (exact + whitespace + indent) | 9 (full chain) | OpenDev | 1 | Critico |
| Stale-read detection | Sim | Sim, <50ms | OpenDev FileTimeTracker | Nao | Alto |
| Tool result summarization | Per-type summaries | + overflow files | OpenDev | Nao | Critico |
| Lazy tool discovery | Core vs deferred split | + ToolSearch meta-tool | OpenDev + Claude Code | Nao | Alto |
| Parallel read-only tools | Static list | Input-dependent via trait | OpenDev ParallelPolicy | Nao | Alto |
| Max concurrent tools | 5 | 5-10 configurable | OpenDev | 1 | Alto |
| Lifecycle hook events | 6 (Pre/Post ToolUse + Session Start/End + Stop + PreCompact) | 10-12 | OpenDev + Claude Code | 0 | Critico |
| Tool categories | 6+ | 12 (enum-based) | OpenDev ToolCategory | 4 (ToolExposure) | Medio |
| Schema description quality | Purpose + Parameters | + Examples + Constraints | arXiv:2602.14878 | Nao auditado | Desconhecido |
| Truncation rules per-tool | 5 tool types | 15+ com 3 strategies | OpenDev sanitizer | Parcial | Medio |
| Middleware pipeline | Before + After hooks | + Abort capability | OpenDev ToolMiddleware | Nao | Alto |
| Dedup cache per-turn | Sim | + skip_dedup() override | OpenDev | Nao | Medio |
| Tool validation errors | LLM-friendly messages | + per-tool custom format | OpenDev format_validation_error | Basico | Medio |

---

## 12. Relevancia para Theo Code

### 12.1 Mapeamento de Findings para Crates

| Finding | Crate Alvo | Prioridade | Complexidade |
|---------|-----------|-----------|-------------|
| Fuzzy edit matching (3+ passes) | `theo-tooling` (edit/mod.rs) | **CRITICA** | Media |
| Tool result summarization | `theo-tooling` (novo modulo summarizer) | **CRITICA** | Baixa |
| Lifecycle hooks (Pre/PostToolUse) | `theo-agent-runtime` | **CRITICA** | Media |
| Lazy tool discovery (ToolSearch) | `theo-infra-mcp` + `theo-tooling` | **ALTA** | Media |
| Parallel read-only tools | `theo-agent-runtime` (agent_loop) | **ALTA** | Media |
| BaseTool trait enrichment | `theo-tooling` (ToolDef trait) | **ALTA** | Baixa |
| Category-based dispatch | `theo-tooling` (registry) | **ALTA** | Baixa |
| Middleware pipeline | `theo-tooling` (novo modulo middleware) | **MEDIA** | Baixa |
| Overflow file storage | `theo-tooling` (truncate/mod.rs) | **MEDIA** | Baixa |
| Dedup cache per-turn | `theo-agent-runtime` | **MEDIA** | Baixa |
| Schema quality audit | `theo-tooling` (todas as tools) | **MEDIA** | Baixa |
| Stale-read detection | `theo-tooling` (edit + read) | **MEDIA** | Baixa |

### 12.2 Implementacao Sugerida por Fase

**Fase 1 -- Quick Wins (1-2 semanas):**
1. Tool result summarizer: modulo novo em `theo-tooling/src/summarizer.rs`, ~150 LOC
2. Schema quality audit: revisar descriptions dos 72 tools contra 6 componentes
3. Category enum: adicionar `ToolCategory` ao `ToolDef` trait

**Fase 2 -- Core Improvements (2-4 semanas):**
4. Fuzzy edit matching: 5-pass chain em `theo-tooling/src/edit/mod.rs`
5. Lifecycle hooks: PreToolUse/PostToolUse em `theo-agent-runtime`
6. Parallel read-only tools: `ParallelPolicy` em `theo-agent-runtime`

**Fase 3 -- Advanced Features (4-6 semanas):**
7. Lazy tool discovery: ToolSearch meta-tool + core/deferred split
8. Middleware pipeline: before/after hooks com abort capability
9. Overflow file storage + stale-read detection
10. Input-dependent concurrency classification

### 12.3 Anti-Patterns a Evitar

| Anti-Pattern | Evidencia | Alternativa SOTA |
|-------------|-----------|------------------|
| Todos os tools no contexto sempre | 40% context waste (OpenDev) | Lazy discovery |
| Exact-match-only edit | 15-20% failure rate | 5-9 pass fuzzy chain |
| Tool output completo no contexto | 70-80% context consumido por outputs | Summarize + overflow |
| Lista hardcoded de read-only tools | Brittle, nao escala | Trait method `is_read_only(args)` |
| Runtime permission blocks | LLM tenta de novo | Schema gating (invisivel) |
| Classificacao de tool estatica | Nao captura Bash reads vs writes | Input-dependent via args |
| Descricoes vagas ("does stuff") | 72% -> 20% selection accuracy | 6 componentes obrigatorias |
| Sem dedup per-turn | Waste em tools identicos | MD5 hash do `(name, args)` |

---

## 13. Citacoes

1. Bui, N. D. Q. (2026). "Building AI Coding Agents for the Terminal: Scaffolding, Harness, Context Engineering, and Lessons Learned." arXiv:2603.05344v1.

2. Liu, J., Zhao, X., Shang, X., & Shen, Z. (2026). "Dive into Claude Code: The Design Space of Today's and Future AI Agent Systems." arXiv:2604.14228.

3. Hasan, M. M., Li, H., Rajbahadur, G. K., Adams, B., & Hassan, A. E. (2026). "Model Context Protocol (MCP) Tool Descriptions Are Smelly! Towards Improving AI Agent Efficiency with Augmented MCP Tool Descriptions." arXiv:2602.14878.

4. arXiv:2602.18914 (2026). "From Docs to Descriptions: Smell-Aware Evaluation of MCP Server Descriptions." (10,831 MCP servers, 18 smell categories).

5. Yao, S. et al. (2024). "tau-bench: A Benchmark for Tool-Agent-User Interaction in Real-World Domains." arXiv:2406.12045.

6. Patil, S. et al. (2025). "The Berkeley Function Calling Leaderboard (BFCL): From Tool Use to Agentic Evaluation of Large Language Models." ICML 2025 (PMLR 267).

7. arXiv:2602.00933 (2026). "MCP-Atlas: A Large-Scale Benchmark for Tool-Use Competency with Real MCP Servers."

8. arXiv:2505.03275 (2025). "RAG-MCP: Mitigating Prompt Bloat in LLM Tool Selection via Retrieval-Augmented Generation."

9. arXiv:2603.20313 (2026). "Semantic Tool Discovery for Large Language Models: A Vector-Based Approach to MCP Tool Selection."

10. OpenDev source code: `crates/opendev-tools-core/src/` (BaseTool trait, ToolRegistry, ParallelPolicy, ToolResultSanitizer, ToolMiddleware).

11. Claude Code hooks documentation: https://code.claude.com/docs/en/hooks

12. BFCL leaderboard: https://gorilla.cs.berkeley.edu/leaderboard.html

13. tau-bench leaderboard: https://taubench.com/

14. MCP Specification 2025-11-25: https://modelcontextprotocol.io/specification/2025-11-25

15. 2026 MCP Roadmap: https://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/
