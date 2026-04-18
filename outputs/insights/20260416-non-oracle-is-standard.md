---
type: insight
topic: "oracle vs non-oracle mode for publication"
confidence: 0.95
impact: high
---

# Insight: Non-oracle e o unico modo aceitavel para publicacao

**Key finding:** Desde mid-2024, todo paper serio reporta non-oracle como resultado primario. Oracle mode (fornecer nomes de testes FAIL_TO_PASS ao agente) infla scores em 15-30% e e considerado "information leakage" pela comunidade.

**Evidence:** SWE-Agent v2 e OpenHands reportam exclusivamente non-oracle. Agentless menciona oracle apenas em apendice. O leaderboard oficial de SWE-bench prioriza non-oracle. O README atual do Theo ja documenta a distincao e tem a flag --no-oracle implementada.

**Implication for Theo:** O resultado atual de 50% (memory file) provavelmente e oracle mode. O numero non-oracle pode ser significativamente menor (estimativa: 35-40%). O plano de 10 fases deve ter como P0 a medicao non-oracle real para calibrar expectativas antes de otimizar.
