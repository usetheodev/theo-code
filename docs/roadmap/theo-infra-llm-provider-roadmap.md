# Roadmap: Providers de Primeira Classe no `theo-infra-llm`

**Escopo:** elevar `crates/theo-infra-llm` de conversor de protocolos para camada real de providers LLM.
**Criado:** 2026-04-01
**Ultima atualizacao:** 2026-04-01

---

## Visao Geral

Hoje o crate cobre bem conversao entre formatos, mas nao cobre a maior parte dos providers concretos suportados pelo sistema principal.
Este roadmap fecha essa lacuna em fases incrementais, com DoD explicito e gate go/no-go entre fases.

```
Fase 1 ──gate──► Fase 2 ──gate──► Fase 3 ──gate──► Fase 4 ──gate──► Fase 5 ──gate──► Fase 6 ──gate──► Fase 7
Arquitetura       Infra comum      Providers base   Providers OA     Providers      Catalogo          Hardening +
e contratos       e resolucao      high-priority    compatibles      especiais      e variantes       rollout
```

---

## Estado Atual

### O que ja existe

- Conversao entre 3 formatos:
  - `Anthropic`
  - `OpenAI`
  - `OaCompat`
- Cliente HTTP centrado em OpenAI-compatible chat completions
- Caso especial de Codex Responses API
- Parsing e normalizacao de streaming SSE

### O que falta

- Registry de providers reais
- Resolucao de auth/env/config por provider
- Endpoint builders por provider
- Clients concretos para Bedrock, Vertex, Azure, GitLab, Copilot etc.
- Catalogo de modelos/providers e filtros de compatibilidade
- Variants, discovery e regras provider-specific hoje espalhadas no app

### Meta de paridade

Suportar como providers de primeira classe, no minimo:

- `openai`
- `openai-compatible`
- `anthropic`
- `azure`
- `azure-cognitive-services`
- `amazon-bedrock`
- `google-vertex`
- `google-vertex-anthropic`
- `openrouter`
- `xai`
- `mistral`
- `groq`
- `deepinfra`
- `cerebras`
- `cohere`
- `togetherai`
- `perplexity`
- `vercel`
- `github-copilot`
- `gitlab`
- `cloudflare-workers-ai`
- `cloudflare-ai-gateway`
- `sap-ai-core`
- `zenmux`
- `kilo`

---

## Fase 1: Reenquadrar Arquitetura

**Objetivo:** separar nitidamente formatos de protocolo, transporte HTTP e providers concretos.

### Entregas

| # | Entrega | Arquivo/modulo alvo |
|---|---|---|
| 1.1 | ADR curta ou doc arquitetural do novo desenho | `docs/current/08-llm-client.md` ou ADR nova |
| 1.2 | Modulo `protocols/` extraido do atual `providers/` | `crates/theo-infra-llm/src/protocols/` |
| 1.3 | Modulo `providers/` redefinido para providers concretos | `crates/theo-infra-llm/src/providers/` |
| 1.4 | Traits centrais | `provider.rs` / `registry.rs` / `resolver.rs` |
| 1.5 | Tipos centrais de identificacao e config | `types.rs` ou `provider_types.rs` |

### Contratos a introduzir

- `ProviderId`
- `ProviderConfig`
- `ResolvedProvider`
- `ProviderSpec`
- `ProviderClient`
- `ProviderResolver`
- `AuthStrategy`
- `EndpointStrategy`

### Definition of Done

- [ ] O termo "provider" passa a significar provider concreto, nao formato de protocolo
- [ ] O codigo atual de conversao fica em namespace/modulo separado de providers concretos
- [ ] Existe um trait `ProviderClient` com interface minima para request normal e streaming
- [ ] Existe um tipo `ProviderId` tipado e usado nas APIs novas
- [ ] Existe um `ProviderRegistry` vazio ou parcial, mas compilando
- [ ] O crate compila sem regressao publica nao documentada
- [ ] Testes do conversor atual continuam verdes
- [ ] Documento arquitetural atualizado explicando a separacao entre protocol, transport e provider

### Gate para Fase 2

- [ ] DoD completo da Fase 1
- [ ] A arquitetura foi revisada e aprovada
- [ ] Nao ha dependencias ciclicas entre `protocols`, `transport` e `providers`

---

## Fase 2: Infra Comum de Resolucao

**Objetivo:** criar a base compartilhada para auth, config, endpoints, headers, retries e runtime capability checks.

### Entregas

| # | Entrega | Arquivo/modulo alvo |
|---|---|---|
| 2.1 | `ProviderRegistry` funcional | `src/providers/registry.rs` |
| 2.2 | `AuthStrategy` por tipo | `src/providers/auth/` |
| 2.3 | Resolver de env + config + credenciais | `src/providers/resolver.rs` |
| 2.4 | Builder de endpoint por provider | `src/providers/endpoint.rs` |
| 2.5 | Regras comuns de retries/timeouts/headers | `src/providers/transport.rs` |
| 2.6 | Harness de testes com fixtures e SSE | `tests/fixtures/providers/` |

### Capacidades minimas da infra

- API key bearer
- API key em header customizado
- OAuth token
- AWS credential chain
- GCP ADC token injection
- endpoint override
- headers extras
- timeout por request
- streaming SSE comum

### Definition of Done

- [ ] `ProviderRegistry` consegue registrar e resolver providers por ID
- [ ] `ProviderResolver` consegue montar um provider a partir de config + env + auth
- [ ] Existe suporte funcional para auth bearer, header customizado, AWS chain e GCP ADC
- [ ] Endpoint override e placeholders em URL estao cobertos por testes
- [ ] Timeouts e retries sao configuraveis por provider
- [ ] O harness de testes suporta request/response fixture e SSE fixture
- [ ] Existe structured logging minimo para resolver/transport
- [ ] Minimo de 20 testes cobrindo resolver, auth e endpoint builder

### Gate para Fase 3

- [ ] DoD completo da Fase 2
- [ ] Testes da infra estao estaveis em CI
- [ ] Ja existe capacidade suficiente para implementar providers base sem duplicacao de codigo

---

## Fase 3: Providers Base de Alta Prioridade

**Objetivo:** suportar os providers que desbloqueiam a maior parte do uso real e consolidam as principais familias de integracao.

### Entregas

| # | Provider | Observacoes |
|---|---|---|
| 3.1 | `openai` | Responses API e/ou chat conforme capacidade |
| 3.2 | `openai-compatible` | Base generica para providers compatveis |
| 3.3 | `anthropic` | Messages API com tool use/result |
| 3.4 | `azure` | Endpoint Azure OpenAI |
| 3.5 | `amazon-bedrock` | AWS auth + region logic |
| 3.6 | `google-vertex` | GCP project/location + ADC |

### Requisitos especiais por provider

- `openai`
  - Responses API suportada
  - streaming
  - tool calls
- `anthropic`
  - `x-api-key`
  - `anthropic-version`
  - system blocks
  - tool use/result
- `azure`
  - `base_url` e deployment/resource resolution
  - suporte a diferenca entre chat/responses quando necessario
- `amazon-bedrock`
  - AWS credential chain
  - optional bearer token
  - region prefix rules
  - endpoint custom
- `google-vertex`
  - project/location resolution
  - endpoint por regiao
  - auth via ADC

### Definition of Done

- [ ] `openai`, `openai-compatible`, `anthropic`, `azure`, `amazon-bedrock` e `google-vertex` possuem implementacao concreta no registry
- [ ] Cada provider tem schema/config tipado e documentado
- [ ] Cada provider suporta request normal e streaming
- [ ] Cada provider tem testes de auth + endpoint + request mapping + response mapping
- [ ] Bedrock tem testes especificos de region prefix logic
- [ ] Vertex tem testes especificos de token injection e endpoint regional
- [ ] Anthropic tem testes especificos de tool use/result e system messages
- [ ] OpenAI e Azure cobrem diferencas entre chat/responses quando aplicavel
- [ ] Minimo de 5 testes por provider e 2 testes de integracao cruzada

### Gate para Fase 4

- [ ] DoD completo da Fase 3
- [ ] O crate consegue executar chamadas reais ou mockadas para os 6 providers base
- [ ] Nao ha codigo provider-specific duplicado fora da camada de provider

---

## Fase 4: Providers OpenAI-Compatible de Alto Impacto

**Objetivo:** absorver rapidamente os providers que podem se apoiar no provider base OpenAI-compatible.

### Entregas

| # | Provider | Base |
|---|---|---|
| 4.1 | `openrouter` | `openai-compatible` |
| 4.2 | `xai` | `openai-compatible` ou `openai` |
| 4.3 | `mistral` | `openai-compatible` |
| 4.4 | `groq` | `openai-compatible` |
| 4.5 | `deepinfra` | `openai-compatible` |
| 4.6 | `cerebras` | `openai-compatible` |
| 4.7 | `cohere` | `openai-compatible` quando aplicavel |
| 4.8 | `togetherai` | `openai-compatible` |
| 4.9 | `perplexity` | `openai-compatible` |
| 4.10 | `vercel` | `openai-compatible` |

### Regras desta fase

- Cada provider deve ser implementado como composicao sobre a base `openai-compatible` sempre que possivel
- Diferencas devem ficar em:
  - headers default
  - base URL default
  - auth env var default
  - feature flags/capabilities

### Definition of Done

- [ ] Todos os 10 providers acima estao registrados no `ProviderRegistry`
- [ ] Cada provider tem base URL default e estrategia de auth definidas
- [ ] Cada provider possui pelo menos 3 testes unitarios
- [ ] Providers com headers obrigatorios tem testes especificos
- [ ] Mistral possui cobertura para diferencas de tool-call ID, se necessario no cliente/transforms
- [ ] Nenhum provider desta fase introduz client HTTP duplicado sem justificativa documentada
- [ ] A adicao de um novo provider OpenAI-compatible passa a exigir apenas declaracao + testes + docs

### Gate para Fase 5

- [ ] DoD completo da Fase 4
- [ ] O template/base de provider OpenAI-compatible esta estavel
- [ ] O custo marginal de adicionar novos providers compatveis caiu visivelmente

---

## Fase 5: Providers Especiais e Nao-Triviais

**Objetivo:** suportar os providers cuja logica foge do modelo comum.

### Entregas

| # | Provider | Complexidade principal |
|---|---|---|
| 5.1 | `github-copilot` | alternancia chat/responses/language-model |
| 5.2 | `gitlab` | gateway proprio, feature flags e model discovery |
| 5.3 | `google-vertex-anthropic` | Anthropic sobre infra Vertex |
| 5.4 | `azure-cognitive-services` | endpoint e auth distintos de Azure OpenAI |
| 5.5 | `cloudflare-workers-ai` | auth e endpoint proprios |
| 5.6 | `cloudflare-ai-gateway` | formato `provider/model`, gateway wrapper |
| 5.7 | `sap-ai-core` | service key e deployment/resource group |
| 5.8 | `zenmux` | headers/base URL especificos |
| 5.9 | `kilo` | headers/base URL especificos |

### Definition of Done

- [ ] Cada provider especial possui implementacao dedicada, sem hacks soltos no client generico
- [ ] `github-copilot` suporta decisao por modelo entre chat/responses quando aplicavel
- [ ] `gitlab` possui auth, headers e model selection implementados
- [ ] `google-vertex-anthropic` reaproveita o maximo possivel de Vertex + Anthropic sem duplicacao indevida
- [ ] `cloudflare-ai-gateway` suporta modelo no formato `provider/model`
- [ ] `sap-ai-core` possui cobertura de env vars e auth strategy
- [ ] Cada provider desta fase possui pelo menos 5 testes
- [ ] Todos os comportamentos especiais estao documentados em comments curtos ou docs publicas do crate

### Gate para Fase 6

- [ ] DoD completo da Fase 5
- [ ] Providers especiais nao degradaram a simplicidade da API publica
- [ ] Os pontos de extensao para futuros providers especiais estao claros

---

## Fase 6: Catalogo, Model Discovery e Variants

**Objetivo:** aproximar a experiencia do crate da logica do sistema principal para listagem, descoberta e selecao de modelos.

### Entregas

| # | Entrega | Arquivo/modulo alvo |
|---|---|---|
| 6.1 | Catalogo local de providers/modelos | `src/catalog/` |
| 6.2 | Cache/snapshot de `models.dev` | `src/catalog/models_dev.rs` |
| 6.3 | Filtros `deprecated/alpha/blacklist/whitelist` | `src/catalog/filter.rs` |
| 6.4 | Variants por modelo | `src/catalog/variants.rs` |
| 6.5 | APIs de listagem e default model | `src/catalog/api.rs` |

### Features alvo

- listar providers disponiveis
- listar modelos por provider
- resolver modelo default
- aplicar whitelist/blacklist
- filtrar deprecated/alpha
- expor variants
- permitir overrides locais de catalogo

### Definition of Done

- [ ] O crate lista providers/modelos sem depender de codigo externo ao crate
- [ ] Existe cache ou snapshot utilizavel offline
- [ ] Filtros `deprecated`, `alpha`, `whitelist` e `blacklist` estao cobertos por testes
- [ ] Variants podem ser consultadas programaticamente
- [ ] Existe API para obter default model por provider
- [ ] Existe API para resolver provider+model a partir de config consolidada
- [ ] Minimo de 15 testes cobrindo catalogo, filtro e variants

### Gate para Fase 7

- [ ] DoD completo da Fase 6
- [ ] O catalogo suporta os providers implementados sem divergencia evidente
- [ ] A integracao com config/env nao introduziu acoplamento circular

---

## Fase 7: Hardening, Observabilidade e Rollout

**Objetivo:** tornar a camada pronta para uso como infraestrutura central.

### Entregas

| # | Entrega | Arquivo/modulo alvo |
|---|---|---|
| 7.1 | Telemetria minima por provider | logging/metrics |
| 7.2 | Retry policy por classe de erro | `transport.rs` |
| 7.3 | Timeout policy por operacao | `transport.rs` |
| 7.4 | Error taxonomy consistente | `error.rs` |
| 7.5 | Testes de conformidade cross-provider | `tests/conformance/` |
| 7.6 | Guia de migracao e rollout | `docs/current/08-llm-client.md` ou doc nova |

### Definition of Done

- [ ] Erros de auth, endpoint, timeout, parse e rate limit possuem tipos claros
- [ ] Existe minimo de logging estruturado para request path, provider ID e tipo de falha
- [ ] Retries ocorrem apenas para classes de erro seguras
- [ ] Timeouts por operacao sao configuraveis e testados
- [ ] Existe suite de conformidade rodando os mesmos cenarios para multiplos providers
- [ ] Existe guia claro para consumidores migrarem do client atual para a API nova
- [ ] Existe pelo menos 1 benchmark basico ou medicao documentada de overhead
- [ ] O rollout plan define providers GA, beta e experimentais

### Gate de Conclusao

- [ ] Todas as fases anteriores concluidas
- [ ] API publica estabilizada
- [ ] Providers base e de alto impacto podem ser usados sem codigo ad hoc fora do crate
- [ ] O crate pode ser adotado como camada central de providers do sistema

---

## Estrategia de Implementacao

### Ordem recomendada

1. Fase 1: separar conceitos e contratos
2. Fase 2: construir resolver/auth/endpoint/transport
3. Fase 3: entregar providers base
4. Fase 4: escalar providers OpenAI-compatible
5. Fase 5: atacar casos especiais
6. Fase 6: trazer catalogo e variants
7. Fase 7: hardening e rollout

### Estrategia de branch/milestone

- 1 milestone por fase
- 1 epic por provider group
- PRs pequenas, cada uma com:
  - 1 provider ou 1 capability transversal
  - testes
  - docs

---

## Matriz de Prioridade

| Prioridade | Itens |
|---|---|
| P0 | `openai`, `openai-compatible`, `anthropic`, `azure`, `amazon-bedrock`, `google-vertex` |
| P1 | `openrouter`, `xai`, `mistral`, `groq`, `deepinfra`, `cerebras`, `cohere`, `togetherai`, `perplexity`, `vercel` |
| P2 | `github-copilot`, `gitlab`, `google-vertex-anthropic`, `azure-cognitive-services`, `cloudflare-workers-ai`, `cloudflare-ai-gateway`, `sap-ai-core` |
| P3 | `zenmux`, `kilo`, demais providers de nicho ou wrappers simples |

---

## Anti-patterns a Evitar

| Anti-pattern | Risco | Alternativa correta |
|---|---|---|
| Continuar chamando formatos de "providers" | Arquitetura confusa | Separar `protocols` de `providers` |
| Colocar regras especiais no client generico | Crescimento caotico | Implementar adapters/specs por provider |
| Duplicar client HTTP para cada provider | Alto custo de manutencao | Base comum de transport + overrides |
| Resolver env/config no call-site | Divergencia entre consumidores | `ProviderResolver` central |
| Misturar catalogo com auth runtime | Acoplamento excessivo | Catalogo separado da resolucao de auth |
| Adicionar provider sem testes de endpoint/auth | Regressao silenciosa | DoD por provider com testes obrigatorios |
| Portar toda a logica do app de uma vez | Escopo explode | Migracao por fases e por grupos de provider |

---

## Criterios Finais de Sucesso

O roadmap sera considerado concluido quando:

1. `theo-infra-llm` suportar providers concretos via registry e resolver padronizado.
2. Os providers P0 e P1 estiverem operacionais com testes e docs.
3. Casos especiais relevantes estiverem encapsulados em implementacoes dedicadas.
4. O crate oferecer listagem e resolucao de providers/modelos sem depender de logica espalhada no app.
5. O client atual OpenAI-centric puder ser tratado como camada legada ou compat wrapper.
