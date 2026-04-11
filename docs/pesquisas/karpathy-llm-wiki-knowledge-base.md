# Karpathy's LLM Wiki: Build a Knowledge Base From Scratch

> Fonte: [YouTube - zVEb19AwkqM](https://www.youtube.com/watch?v=zVEb19AwkqM)

---

## O Problema com RAG Tradicional

A experiência atual da maioria das pessoas com LLMs e documentos se resume a RAG (Retrieval Augmented Generation): você faz upload de arquivos, faz uma pergunta, o LLM recupera chunks e gera uma resposta.

**O problema fundamental:** nada se acumula. A cada pergunta, o LLM redescobre conhecimento do zero, remontando fragmentos toda vez. Se você faz uma pergunta sutil que requer sintetizar cinco documentos diferentes, ele precisa encontrar e conectar todas as peças em cada query. Não há memória, cross-references, nem entendimento acumulado.

---

## O Padrão LLM Wiki

O padrão LLM Wiki inverte a lógica: em vez de recuperar no momento da query, o LLM constrói uma wiki persistente e interligada antecipadamente.

> "The LLM incrementally builds and maintains a persistent wiki — structured, interlinked markdown files sitting between you and your raw sources."
> — Andrej Karpathy

**Princípio central:** Você nunca escreve a wiki. O LLM escreve e mantém tudo. Você cuida do que importa: encontrar boas fontes, explorar, fazer as perguntas certas. O LLM cuida do trabalho braçal — resumir, cross-referenciar, arquivar, manter consistência.

---

## Arquitetura de 3 Camadas

### 1. Raw Sources (Esquerda)

Artigos, papers, imagens, datasets — qualquer material coletado. São **imutáveis**: o LLM lê mas nunca altera. São a fonte de verdade.

### 2. Wiki (Centro)

Diretório de arquivos markdown que o LLM controla inteiramente:
- Summaries
- Entity pages
- Concept pages
- Comparisons
- Cross-references

O LLM cria, atualiza quando novas fontes chegam e mantém tudo consistente.

### 3. Schema (Direita)

Arquivo de configuração (como um `CLAUDE.md`) que diz ao LLM:
- Como a wiki é estruturada
- Quais convenções seguir
- Quais workflows executar

Você e o LLM co-evoluem este schema ao longo do tempo.

**Analogia:** A wiki é o codebase, Obsidian é a IDE, o LLM é o programador, e o schema é o style guide.

---

## 3 Operações Core

### 1. Ingest

Você coloca uma nova fonte na pasta `raw/` e pede ao LLM para processá-la:

1. Lê a fonte raw
2. Extrai informações-chave: conceitos, entidades, claims, data points
3. Escreve uma summary page na wiki com metadata e tags
4. Atualiza todas as entity/concept pages existentes, integrando nova informação
5. Sinaliza contradições quando novos dados conflitam com claims existentes
6. Atualiza o index (catálogo master de tudo na wiki)
7. Adiciona ao log (registro timestamped do que mudou e quando)

> Uma única fonte pode tocar 10 a 15 páginas da wiki. O efeito é compounding: cada fonte torna a wiki inteira mais inteligente.

### 2. Query

Você faz perguntas contra a wiki. O LLM pesquisa o index, lê as páginas relevantes e sintetiza uma resposta.

**Parte inteligente:** Boas respostas podem ser arquivadas de volta na wiki como novas páginas. Suas explorações compõem na knowledge base assim como fontes ingeridas.

Se o LLM não encontrar a resposta na wiki, ele pode fazer web search e automaticamente **backfill** a wiki com a informação nova.

### 3. Lint

Passe de manutenção onde o LLM faz health check da wiki:
- Encontra contradições
- Identifica claims desatualizadas
- Detecta orphan pages (sem links)
- Aponta cross-references faltantes
- Sugere gaps que podem ser preenchidos com web search
- Sugere novas perguntas para investigar

---

## Divisão de Trabalho

| Humano | LLM |
|---|---|
| Escolhe as fontes | Escreve todas as páginas da wiki |
| Dirige a análise | Mantém cross-references atualizadas |
| Faz as boas perguntas | Mantém summaries |
| Decide o que importa | Sinaliza contradições |
| Curadoria e pensamento crítico | Summarizar, referenciar, arquivar |

> "Humans abandon wikis because the maintenance burden grows faster than the value. LLMs don't get bored. They don't forget to update a cross reference. They can touch 15 files in a single pass. The cost of maintenance drops to near zero."
> — Karpathy

---

## 4 Princípios que Fazem Funcionar

1. **Explicit** — O conhecimento é visível em uma wiki navegável. Você vê exatamente o que a IA sabe e o que não sabe. Sem embeddings ocultos, sem sistema de memória opaco.

2. **Yours** — São arquivos locais no seu computador. Sem lock-in de provider. Você é dono de tudo.

3. **File over App** — Tudo em formatos universais (Markdown + imagens). Interoperável com qualquer tool, CLI ou viewer. Todo o toolkit Unix funciona.

4. **Bring Your Own AI** — Funciona com Claude, GPT, Codex, modelos open source. Você pode até fine-tunar um modelo na sua wiki para que ele conheça seus dados nos weights, não apenas no contexto.

---

## Casos de Uso

| Domínio | Descrição |
|---|---|
| **Research** | Aprofundar em tópico ao longo de semanas/meses, lendo papers, construindo wiki com tese evolutiva |
| **Personal** | Rastrear objetivos, saúde, auto-melhoria — picture estruturada de si mesmo ao longo do tempo |
| **Business** | Wiki interna alimentada por Slack, meetings, calls de clientes — sempre atualizada |
| **Writing** | Construir capítulos de livro, personagens e temas |
| **Due Diligence** | Pesquisa aprofundada sobre empresas, mercados, oportunidades |
| **Trading** | Estratégias, conceitos de mercado, análise técnica (exemplo do vídeo) |

---

## Estrutura de Diretórios

```
project/
├── raw/                    # Fontes brutas (imutáveis)
│   ├── transcript_1.txt
│   ├── transcript_2.txt
│   └── ...
├── wiki/                   # Wiki gerada pelo LLM
│   ├── index.md            # Catálogo master
│   ├── concepts/           # Páginas de conceitos
│   ├── entities/           # Páginas de entidades
│   ├── summaries/          # Resumos de fontes
│   ├── synthesis/          # Comparações, trade-offs
│   └── log.md              # Registro de mudanças
└── CLAUDE.md               # Schema + workflows
```

---

## Como Construir na Prática (com Claude Code)

1. **Preparar raw sources** — Copiar transcrições/documentos para a pasta `raw/`
2. **Escrever o schema** (`CLAUDE.md`) — Definir estrutura, convenções, Obsidian-style wiki links
3. **Scaffolding** — Criar estrutura de diretórios e seed de concept pages
4. **Ingest em batch** — Rodar agentes para ingerir todas as fontes, criar summaries e atualizar concept pages
5. **Visualizar em Obsidian** — Abrir o diretório como vault para navegar links e estrutura
6. **Iterar** — Fazer perguntas, deixar o LLM pesquisar e backfill, rodar lint periodicamente

**Tempo estimado para versão básica:** ~1 hora no Claude Code, depois customizar conforme necessidade.

---

## Ciclo de Crescimento da Wiki

```
Pergunta → LLM busca na wiki → Resposta
                                    ↓
                            Informação suficiente?
                           /                    \
                         Sim                     Não
                          ↓                       ↓
                   Responde direto          Web search
                                              ↓
                                     Backfill na wiki
                                              ↓
                                    Novas concept pages
                                              ↓
                                    Wiki fica mais smart
```

> "You ask a question, I research beyond the wiki, and the new knowledge gets filed back as permanent pages. Every future query can now reference the new concepts along with the original stuff."

---

## Ferramentas Mencionadas

- **Claude Code** com Opus 4.6 — Usado para construir a wiki no vídeo
- **Obsidian** — IDE para visualizar e navegar a wiki (wiki links, graph view)
- **Qualquer LLM** — OpenAI, Claude, modelos open source, Hermes Agent
- **Markdown** — Formato universal para todo o conteúdo
