---
type: insight
topic: "MCP and A2A protocol convergence"
confidence: 0.78
impact: medium
---

# Insight: MCP (Vertical) + A2A (Horizontal) = The Agent Communication Standard

**Key finding:** The industry is converging on two complementary protocols: MCP for agent-to-tool communication (vertical) and A2A for agent-to-agent communication (horizontal). Both are now under the Linux Foundation. The recommended adoption path is MCP first, then A2A.

**Evidence:** MCP has 97M+ monthly SDK downloads and adoption by OpenAI, Google DeepMind, Microsoft. Donated to Linux Foundation in December 2025. A2A has 150+ organizations and v1.0 with production deployments at Microsoft, AWS, Salesforce, SAP. IBM's ACP merged into A2A in August 2025. arXiv survey (2505.02279) proposes phased adoption: MCP for tool access, ACP for multimodal messaging, A2A for collaborative task execution, ANP for decentralized marketplaces.

**Implication for Theo:** Implementing Theo as an MCP server (exposing Theo's tools via MCP) should be a near-term priority. This enables Theo to be composed into multi-agent workflows orchestrated by other systems. A2A is relevant for the future but not urgent — Theo is not yet in a position where it needs to communicate with peer agents at the protocol level. The existing `tool_manifest.rs` infrastructure can serve as the foundation for MCP server mode.
