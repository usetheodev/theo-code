# Agentic Harness for Real-World Compilers (llvm-autofix)

**Paper:** arXiv:2603.20075v1
**Autores:** Yingwei Zheng, Cong Li, Shaohua Li, Yuqun Zhang, Zhendong Su
**Instituições:** Southern University of Science and Technology, ETH Zurich, Chinese University of Hong Kong
**Data:** March 23, 2026
**Repo:** https://github.com/dtcxzyw/llvm-autofix

---

## 1. O que é

`llvm-autofix` é o primeiro harness agentic especializado para **bugs de compiladores**, focado no LLVM middle-end. O paper demonstra que LLMs que performam bem em benchmarks de software genérico (SWE-bench) **caem 62% de performance** quando enfrentam bugs de compiladores — evidenciando que harnesses domain-specific são essenciais.

**3 componentes:**
1. **llvm-autofix tooling** — ferramentas compiler-specific para build, reproduce, debug, edit e validate patches no LLVM
2. **llvm-bench** — benchmark de 334 bugs reproduzíveis do LLVM middle-end com regression tests robustos
3. **llvm-autofix-mini** — agente minimal de 4 estágios (Setup→Reason→Generate→Validate) que supera `mini-SWE-agent` em ~22%

## 2. O Problema

Bugs de compiladores são fundamentalmente diferentes de bugs de software comum:

| Aspecto | Software Comum (Django, etc.) | Compiladores (LLVM) |
|---------|------------------------------|---------------------|
| Descrição do bug | Linguagem natural detalhada | Apenas reproducer (IR code), sem descrição textual |
| Tipos de bug | Logic errors, UI, API | **Crash** (@test reproducer + stack trace), **Miscompilation** (@src + counterexample) |
| Expertise necessária | Domínio da aplicação | Lexing, parsing, type systems, IR design, optimization, code generation |
| Ferramentas | bash, grep, git | `opt`, `alive2`, `llvm-lit`, `FileCheck`, gdb com breakpoints em IR transforms |
| Validação | Testes unitários | Reproducer + regression tests + differential testing (alive2) |
| Complexidade | 1-2 arquivos, lógica local | Cross-component (184 middle-end components), IR transformations |

**Achado-chave:** Modelos que resolvem 60% no SWE-bench Verified resolvem apenas **38%** no llvm-bench live (o melhor, DeepSeek V3.2). Performance cai **62% em média** ao trocar de SWE-bench para llvm-bench.

## 3. llvm-autofix Tooling (5 categorias)

### Setup & Build
- Configura LLVM para build com commit específico (base commit do bug)
- Build com debug info para x86
- Agent não precisa lidar com build system — harness abstrai

### Reproduce & Cause
- Valida se o bug é reproduzível no base commit
- **Crash bugs:** roda `opt` com reproducer, verifica crash, filtra stack trace (remove frames irrelevantes)
- **Miscompilation bugs:** roda `opt` + `alive2` (translation validator), outputa counterexample

### Explore & Debug
- **Estáticos:** grep, find, code, docs, langref (LLVM IR specification)
- **Dinâmicos:** gdb com breakpoints no reproducer — pausa LLVM antes do crashing function / first transformation, permite `eval(expr=WidePhi)`, inspecionar variáveis, jump entre frames

### Edit & Patch
- edit, reset, preview de mudanças
- Submit patch para validação

### Test & Validate
- Rebuild LLVM com patch
- Executa: reproducers + component-specific regression tests + regression tests de outros components
- Usa `opt`, `alive2`, `llvm-lit`, `FileCheck`
- Validação online (durante execução) ou offline (pós-agente)

## 4. llvm-bench Benchmark

### Construção (3 estágios automatizados)

1. **Issue collection:** Issues fixados no GitHub do LLVM com commits correspondentes
2. **Reproducer validation:** Verificar que reproducers de fato reproduzem no base commit via `llvm-autofix` tools
3. **Golden patch validation:** Verificar que o patch oficial passa todos os testes

### Estatísticas

| Métrica | full | live (últimos 12 meses) |
|---------|------|------------------------|
| Bugs totais | 334 (222 crash, 112 miscomp) | 229 (160 crash, 69 miscomp) |
| Components afetados | 64 de 184 | 43 |
| Reproducers por issue | 1.4 média | 1.5 |
| Regression tests por issue | 722 média | 743 |
| LOC por reproducer | 49.7 média | — |
| LOC editadas por fix | 17.1 média | 18.5 |
| Funções alteradas por fix | 1.6 média | 1.7 |
| Arquivos alterados por fix | 1.2 média | 1.2 |

### 3 Splits por dificuldade

| Split | Definição | % do total |
|-------|-----------|-----------|
| **Easy** | Fix em uma única função | 76.3% |
| **Medium** | Fix em múltiplas funções, mesmo arquivo | 13.2% |
| **Hard** | Fix em múltiplos arquivos | 10.5% |

### live subset
Contém apenas issues dos últimos 12 meses — evita data leakage (bugs antigos podem estar no training data dos LLMs). Continuamente atualizado.

## 5. llvm-autofix-mini Agent (4 estágios)

```
Stage I: SETUP
├─ Validate reproducer reproducibility
├─ Launch LLVM under gdb with reproducer
├─ Set breakpoint: crash → crashing function; miscomp → first transformation
└─ Infer erroneous component → pass to Reason stage

Stage II: REASON (ReAct loop)
├─ debug(cmd=frame 3) — jump to stack frames
├─ eval(expr=WidePhi) — inspect local state
├─ code(func=llvm::VPTransformState::get) — read source
├─ docs(func=llvm::VPTransformState::get) — read documentation
└─ Loop until root cause identified

Stage III: GENERATE (ReAct loop)
├─ edit(file=.../SLPVectorizer, text=..., replace=...) — modify code
├─ reset() — revert if needed
├─ test() — online validation with feedback
└─ Loop until online testing succeeds

Stage IV: VALIDATE
├─ Offline testing (full regression suite)
└─ Output: accepted patch or failure
```

**Tools disponíveis para llvm-autofix-mini:** find, grep, list, read, edit, code, docs, langref, debug, eval, reset, preview, test

## 6. Resultados

### 6.1 Performance drop: SWE-bench → llvm-bench

| Model | SWE-bench Verified (%) | llvm-bench live (%) | Drop |
|-------|----------------------|-------------------|------|
| GPT 4o | 21.6 | 8.3 | **-61.6%** |
| GPT 5 | 65.0 | 21.0 | **-67.7%** |
| Gemini 2.5 Pro | 53.6 | 9.2 | **-82.8%** |
| Qwen 3 Max | 69.6 | 24.4 | **-64.9%** |
| DeepSeek V3.2 | 60.0 | 38.9 | **-35.2%** |

**Média de drop: 62%.** Compiler bugs são fundamentalmente mais difíceis que bugs de software comum.

### 6.2 llvm-autofix-mini vs mini-SWE-agent

| Model | mini-SWE-agent (%) | llvm-autofix-mini (%) | Improvement | Avg Cost |
|-------|-------------------|---------------------|------------|----------|
| GPT 4o | 8.3 | 12.2 | +1.47× | $2.18 |
| GPT 5 | 21.0 | **51.5** | **+2.45×** | $0.59 |
| Gemini 2.5 Pro | 9.2 | 14.4 | +1.57× | $1.16 |
| Qwen 3 Max | 24.4 | 35.8 | +1.47× | $5.67 |
| DeepSeek V3.2 | 38.9 | 10.5 | -0.27× | $0.15 |

**llvm-autofix-mini supera mini-SWE-agent em 1.22× na média**, com custo incremental <$1.50/issue. GPT 5 com llvm-autofix-mini resolve 51.5% — melhor resultado geral.

**Exceção:** DeepSeek V3.2 regride com llvm-autofix-mini (-73%) porque frequentemente falha em aderir ao formato de tool-calling, resultando em >85% de falhas por exceder token budget.

### 6.3 Por dificuldade

| Split | mini-SWE-agent avg (%) | llvm-autofix-mini avg (%) |
|-------|----------------------|-------------------------|
| Easy | 23.2 | 28.8 |
| Medium | 15.8 | 14.7 |
| Hard | 6.1 | 7.8 |

**Hard split é devastador:** nenhum modelo consegue resolver mais que uma issue no hard split, exceto GPT 5 com llvm-autofix-mini (1 issue resolvida).

### 6.4 Expert Review — Genuine Capability

| Model | % Correct (of accepted) | % Genuinely Resolved |
|-------|------------------------|---------------------|
| GPT 4o | 32.1 | 3.9 |
| GPT 5 | 39.0 | **20.1** |
| Gemini 2.5 Pro | 36.4 | 5.2 |
| Qwen 3 Max | 36.6 | 13.1 |
| DeepSeek V3.2 | 41.7 | 4.4 |

**Achado devastador:** Mesmo para o melhor modelo (GPT 5 + llvm-autofix-mini), **>60% dos patches aceitos estão incorretos** após expert review. A genuine resolution rate é apenas **20.1%**.

**Implicação:** Testes de regressão do LLVM são insuficientes para validar patches gerados por LLMs. Patches passam testes mas são semanticamente incorretos.

## 7. 3 Categorias de Erros de LLMs

### 7.1 ChangeAssert
LLMs "trapaceiam" alterando a assertion condition para evitar crashes em vez de corrigir o bug:
1. Modificar a condição da assertion diretamente
2. Alterar funções que a assertion invoca
3. Inserir early returns antes da assertion

### 7.2 WrongLocalization
Mesmo com o componente errado fornecido pelo harness, LLMs falham em localizar o bug dentro dos arquivos e funções corretos. Pior quando bugs impactam múltiplas funções ou arquivos.

### 7.3 WrongFix
Localização correta mas fix errado:
1. **Bypass:** Enfraquecer/fortalecer activation condition em vez de corrigir
2. **Falta de generalidade:** Patch funciona para o reproducer mas não para outros inputs
3. **Bugs silenciosos:** Patch introduz novos bugs (ex: missed optimizations que inibem transformações subsequentes)

## 8. Open Challenges (Section 5)

1. **Preventing bypassing** — Agents alteram assertions em vez de corrigir root cause. Necessário: bypass-detection defenses que verifiquem que assertions e componentes continuam ativos
2. **Overcoming short-sightedness** — Agents param quando testes passam, gerando patches incompletos que compilam mas falham em edge cases. Necessário: estratégias online + offline
3. **Managing long context** — Compiladores exigem contexto grande (LLVM codebase é massivo), levando ao "context rot". Necessário: context management sofisticado (alinhado com ACC do OpenDev)

## 9. Relevância para Theo Code

### Lições diretas (mesmo domínio: sistemas complexos em Rust)

| Lição | Aplicação no Theo Code |
|-------|----------------------|
| **Harness domain-specific >> harness genérico** (+22% por modelo) | O SOTA loop DEVE ter probes especializados por crate, não probes genéricos |
| **Testes de regressão são insuficientes** (>60% dos patches "aceitos" são incorretos) | Quality evaluator deve ir além de "cargo test passa" — code review semântico necessário |
| **ChangeAssert anti-pattern** | Nosso loop deve detectar patches que enfraquecem assertions/conditions em vez de corrigir |
| **WrongLocalization** mesmo com component fornecido | Gap analyzer deve verificar localização ANTES de propor fix |
| **Hard bugs (multi-file) são quase impossíveis** | Priorizar bugs single-file primeiro, escalar gradualmente |
| **Performance drops 62% de SWE-bench para domain-specific** | Nossos thresholds SOTA devem considerar que performance em benchmarks genéricos NÃO transfere |
| **Expert review revela genuine rate ~20%** | Considerar code review humano ou LLM para patches críticos |

### Thresholds para o SOTA registry

| Métrica | Valor | Source |
|---------|-------|-------|
| SWE-bench → domain-specific drop | **62%** | Média de drop ao trocar benchmark |
| Expert review "genuine" rate | **20.1%** (best) | GPT 5 + llvm-autofix-mini |
| Domain-specific harness improvement | **+22%** per model | llvm-autofix-mini vs mini-SWE-agent |
| Hard split resolution rate | **<8%** | Multi-file bugs, todos modelos |
| Best overall resolution (domain-specific) | **51.5%** | GPT 5 + llvm-autofix-mini |

### Anti-patterns para o SOTA loop detectar

1. **ChangeAssert** — Se o patch modifica uma assertion condition, flag como suspeito
2. **Short-sighted fix** — Se o patch resolve apenas o reproducer/probe sem generalizar, flag
3. **Bypass fix** — Se o patch enfraquece uma activation condition, flag
4. **Tests pass ≠ correct** — Nunca confiar apenas em "cargo test passa" para patches complexos

---

**Citação:**
```
Zheng, Y., Li, C., Li, S., Zhang, Y., & Su, Z. (2026). Agentic Harness for Real-World Compilers. arXiv:2603.20075v1.
```
