---
type: insight
topic: "ablation studies for GRAPHCTX publication"
confidence: 0.92
impact: high
---

# Insight: Sem ablation, GRAPHCTX e um claim anecdotico

**Key finding:** Todo paper aceito em 2024-2025 que faz claims sobre um componente especifico (ferramenta, prompt strategy, retrieval method) inclui ablation study removendo esse componente e medindo o delta. Sem isso, revisores rejeitam o claim.

**Evidence:** SWE-Agent faz ablation de 6 ferramentas individualmente. Agentless faz ablation de cada fase do pipeline (localization sozinho = X%, + repair = Y%, + re-rank = Z%). OpenHands compara CodeAct vs alternativas. Nenhum publica "nosso sistema resolve N%" sem mostrar qual parte contribui quanto.

**Implication for Theo:** O plano de 10 fases DEVE incluir ablation como fase bloqueante antes de qualquer publicacao. Configuracao minima: (1) Theo full, (2) Theo sem GRAPHCTX, (3) Theo sem state machine, (4) baseline prompt-only. Rodar no mesmo subset de 100 instancias Verified, mesmo modelo, 3 runs cada. McNemar test para significancia estatistica. Sem isso, o numero de resolve rate e apenas marketing, nao ciencia.
