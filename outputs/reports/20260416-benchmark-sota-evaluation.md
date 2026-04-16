---
type: report
question: "Benchmark overhaul: qual e o estado da arte para publicacao de resultados de agentes de codigo?"
generated_at: 2026-04-16T09:25:00-03:00
confidence: 0.88
sources_used: 6
---

# Report: Benchmark State-of-the-Art para Agentes de Codigo

## Executive Summary

O padrao da comunidade convergiu para SWE-bench Verified (500 instancias) em modo non-oracle como benchmark primario de publicacao. Pass@1 resolve rate com 3+ runs e intervalos de confianca e o metodo aceito. O harness atual do Theo ja tem a infraestrutura certa (adapter.py + grader oficial), mas falta rigor estatistico e ablation methodology para ser publication-grade.

## Analise por Topico

### 1. SWE-bench Verified vs Lite vs Pro

**POSICAO:** Verified e o padrao para publicacao. Lite e aceitavel. Pro ainda nao e exigido.

**ANALISE:** SWE-bench Lite (300 instancias) foi o padrao ate mid-2024, mas tem problemas conhecidos: instancias ambiguas, testes frageis, patches com multiplas solucoes validas que o grader rejeita. SWE-bench Verified (500 instancias curadas por engenheiros) corrigiu esses problemas e se tornou o padrao de facto em papers de 2024-2025 (OpenHands, SWE-Agent v2, Agentless, CodeR). SWE-bench Pro e uma extensao mais recente com instancias de maior complexidade, mas adocao ainda e limitada — poucos agentes reportam nele.

**RISCOS:** (1) Rodar so Lite pode ser visto como cherry-picking se Verified esta disponivel. (2) Pro muda rapido e resultados podem nao ser comparaveis entre versoes.

**RECOMENDACOES:** (1) Publicar com Verified como primario, Lite como secundario para comparabilidade historica. (2) Monitorar Pro mas nao priorizar agora.

### 2. Pass@k vs Resolve Rate

**POSICAO:** Pass@1 resolve rate e o padrao. Pass@k>1 e bonus, nao substituto.

**ANALISE:** A comunidade usa "resolve rate" = porcentagem de instancias onde o patch gerado passa nos testes gold, avaliado pelo grader oficial de Princeton. Pass@1 com temperature=0 e o baseline minimo. Papers mais rigorosos (SWE-Agent, Agentless) reportam Pass@1 com 3-5 runs e media/desvio padrao. Pass@5 e reportado como metrica secundaria para mostrar capacidade maxima, mas nao substitui Pass@1. Ninguem na comunidade usa Pass@k como metrica primaria isolada.

**RISCOS:** (1) Um unico run com temperature=0 ainda tem variancia por nondeterminismo de APIs — um numero nao e confiavel.

**RECOMENDACOES:** (1) Reportar Pass@1 como media de 3 runs com IC 95% (Wilson). (2) Pass@5 como metrica secundaria opcional.

### 3. Oracle vs Non-Oracle

**POSICAO:** Non-oracle e o padrao para publicacao. Oracle e aceitavel para desenvolvimento interno.

**ANALISE:** No modo oracle, o agente recebe os nomes dos testes que devem passar (FAIL_TO_PASS), o que e informacao que um desenvolvedor real nao teria. Papers serios desde mid-2024 reportam non-oracle como resultado primario. SWE-Agent, OpenHands e Agentless todos usam non-oracle para claims principais. Alguns papers reportam ambos para transparencia. O README do Theo ja documenta essa distincao corretamente.

**RISCOS:** (1) Resultados oracle podem ser 15-30% maiores que non-oracle — publicar oracle sem disclosure e desonesto e sera criticado. (2) Non-oracle e significativamente mais dificil para o agente.

**RECOMENDACOES:** (1) Non-oracle como numero primario de publicacao. (2) Oracle como metrica de desenvolvimento/debugging. (3) Sempre disclose.

### 4. Ablation Studies

**POSICAO:** Ablation e obrigatorio para qualquer claim de que um componente (ex: GRAPHCTX) agrega valor.

**ANALISE:** Papers de referencia fazem ablations consistentes:
- **SWE-Agent:** ablation de ferramentas (com/sem search, com/sem edit format), ablation de prompt (com/sem demo, com/sem hints), ablation de modelo (GPT-4, Claude, open-source).
- **OpenHands:** ablation de agente (CodeAct vs planner), ablation de contexto, ablation de max iterations.
- **Agentless:** ablation de cada fase do pipeline (localization, repair, re-ranking), mostrando contribuicao incremental.
- **Aider:** ablation de edit format (diff vs whole-file vs search-replace), ablation de modelo.

Padrao comum: (1) Sistema completo vs sistema sem componente X. (2) Mesmo dataset, mesmo modelo, mesmo seed. (3) Diferenca estatisticamente significativa (McNemar test ou bootstrap CI).

**RISCOS:** (1) Sem ablation, claims sobre GRAPHCTX sao anecdoticos. (2) Ablation mal feita (confounders nao controlados) e pior que nao ter.

**RECOMENDACOES:** (1) Ablation minimo para Theo: {full system} vs {sem GRAPHCTX} vs {sem state machine} vs {baseline prompt-only}. (2) Rodar no mesmo subset de Verified (50-100 instancias e suficiente para ablation).

### 5. Metodos Estatisticos

**POSICAO:** Bootstrap CI + McNemar test e o padrao emergente. Muitos papers ainda sao fracos aqui — oportunidade de diferenciacao.

**ANALISE:** A maioria dos papers de agentes usa metricas simples (resolve rate como porcentagem unica) sem intervalos de confianca — isso e uma fraqueza conhecida da area. Papers mais rigorosos usam:
- **Bootstrap confidence intervals** (95% CI via 1000+ resamples) para resolve rate.
- **McNemar's test** para comparar dois agentes no mesmo dataset (e.g., "GRAPHCTX vs no-GRAPHCTX" instancia por instancia).
- **Wilson score interval** para proporcoes com amostras pequenas (melhor que Wald/normal para n<300).
- **Paired analysis:** report quais instancias cada agente resolve que o outro nao resolve (Venn diagram).

O Theo ja tem Wilson CI implementado em `_headless.py` — isso e um diferencial.

**RISCOS:** (1) Multiplas comparacoes sem correcao (Bonferroni) podem gerar falsos positivos em ablation. (2) Resolve rate e uma metrica binaria por instancia — variancia entre runs e alta.

**RECOMENDACOES:** (1) Usar McNemar para comparacoes pareadas (ablation). (2) Bootstrap CI para resolve rate geral. (3) Reportar paired Venn (quais instancias so o Theo resolve que outros nao).

## Gaps

- **Custo-eficiencia como metrica:** papers recentes (Agentless, MASAI) comecam a reportar cost per resolved instance. O Theo tem cost tracking mas nao normaliza por resolucao.
- **Reprodutibilidade de modelos closed-source:** se usar GPT-4o ou Claude, resultados podem mudar com model updates silenciosos. Papers com modelos open-source sao mais reprodutiveis.
- **Time-to-resolve:** nenhum paper padroniza isso ainda, mas e diferencial potencial.

## Recomendacoes Consolidadas para o Plano de 10 Fases

1. **Dataset primario:** SWE-bench Verified, non-oracle, Pass@1 media de 3 runs.
2. **Ablation minimo:** full vs no-GRAPHCTX vs no-state-machine vs prompt-only. 100 instancias Verified e suficiente.
3. **Estatistica:** Wilson CI (ja tem), McNemar para ablation, bootstrap para comparacoes cruzadas.
4. **Metricas secundarias:** Pass@5, cost-per-resolved, paired Venn vs baselines publicos.
5. **Disclosure obrigatorio:** modelo exato + versao, oracle/non-oracle, temperatura, numero de runs, git commit do binary.

## Sources

1. SWE-bench: Can Language Models Resolve Real-World GitHub Issues? (Jimenez et al., 2024) — dataset original
2. SWE-bench Verified — curated subset by Princeton NLP (2024)
3. SWE-Agent: Agent-Computer Interfaces Enable Automated Software Engineering (Yang et al., 2024)
4. OpenHands: An Open Platform for AI Software Developers (Wang et al., 2024)
5. Agentless: Demystifying LLM-based Software Engineering Agents (Xia et al., 2024)
6. Aider benchmark methodology — aider.chat/docs/benchmarks
