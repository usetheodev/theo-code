# Evolution Research: Context Manager 4.7 → 5.0

**Date:** 2026-04-18
**Prompt:** Analisar e revisar Context Manager usando melhores práticas 2026
**Sources:** OpenDev (Rust), Pi-Mono (TS), awesome-harness-engineering (papers/2026)
**Baseline:** score=48.095 (L1=63.077, L2=33.113)

---

## Current State

Context Manager distributed across 4 crates:
- `theo-domain`: Budget, EpisodeSummary, WorkingSet, MemoryLifecycle, Hypothesis, tokens
- `theo-engine-retrieval`: assembly (greedy knapsack), BudgetConfig
- `theo-application`: ContextAssembler (orchestrator, hard rules, feedback loop)
- `theo-agent-runtime`: compaction, ContextMetrics, BudgetEnforcer, RunSnapshot

Rated 4.7/5 with 5 gaps for 5/5.

---

## Gap 1: Hypothesis Engine

**Current:** Hypothesis is `Option<String>` in WorkingSet with Active/Stale/Superseded. No confidence scoring, no competition, no auto-pruning.

**Patterns:**
- **LATS** [arxiv 2310.04406]: Confidence = accumulated reward, prune below threshold
- **OpenDev DoomLoopDetector** [doom_loop.rs]: Fingerprint + cycle detection → escalating recovery

**Actionable:** Add `confidence: f32` and `evidence_for/against: u32` to Hypothesis. Auto-transition to Stale when `evidence_against > evidence_for * 2`. Simple, no tree search needed.

---

## Gap 2: Memory Typing

**Current:** MemoryLifecycle (Active/Cooling/Archived) but all EpisodeSummary are flat. No kind distinction.

**Patterns:**
- **MemGPT/Letta Three-Tier** [letta-ai/letta]: Core/Recall/Archival
- **Knowledge Objects** [arxiv 2603.17781]: 60% fact destruction during compaction → hash-addressed facts
- **MemArchitect** [arxiv 2603.18330]: Policy on read, not just write; decay + conflict resolution
- **OpenDev ArtifactIndex** [compaction.rs]: File ops survive compaction as canonical memory
- **OpenDev SessionMemoryCollector** [session_memory.rs]: Structured episodic extraction at 50K intervals

**Actionable:** Add `MemoryKind` enum: Ephemeral/Episodic/Reusable/Canonical. Orthogonal to MemoryLifecycle (lifecycle = when to transition, kind = what type of knowledge). Eviction policy varies by kind.

---

## Gap 3: Causal Usefulness

**Current:** ContextMetrics tracks `assembled_chunks` and `tool_references` separately. No linkage.

**Patterns:**
- **OpenDev TurnContext** [attachments/mod.rs]: `cumulative_input_tokens` enables token-budget-driven collection
- **Pi-Mono Dual-Token Estimation** [compaction.ts]: API anchor + heuristic trailing
- **AgentRx** [Microsoft]: Trajectory normalization + constraint synthesis = causal attribution
- **CausalTrace** [derived]: `{turn_id, input_segments, output_action, outcome}` tuples

**Actionable:** Add `CausalLink { community_id, tool_call_id, outcome: CausalOutcome }` to ContextMetrics. Compute usefulness as `successful_references / total_assemblies` per community.

---

## Gap 4: Failure Learning Loop

**Current:** EpisodeSummary captures `failed_attempts` and `learned_constraints` at episode end. No real-time detection.

**Patterns:**
- **OpenDev DoomLoopDetector** [doom_loop.rs]: Fingerprint deque(20), cycle detection (1-3), escalation (Nudge→StepBack→Compact)
- **AgentDebug** [ICLR 2026, arxiv 2509.25370]: Error taxonomy (Memory/Planning/Action/System), +24% accuracy
- **AgentAssay** [arxiv 2603.02601]: Behavioral fingerprinting, 86% regression detection
- **OpenDev SessionMemoryCollector** [session_memory.rs]: "Errors & Corrections" section

**Actionable:** Add `FailureFingerprint { error_class: ErrorClass, tool_name: String, args_hash: u64 }`. Ring buffer in ContextMetrics. When fingerprint recurs ≥3 times → auto-add to WorkingSet.constraints.

---

## Gap 5: Multi-Agent WorkingSet Isolation

**Current:** WorkingSet has `agent_id: Option<String>` and `merge_from()`. No isolation guarantees.

**Patterns:**
- **OpenDev SubAgentSpec** [subagents/spec/types.rs]: IsolationMode::None|Worktree, permission model, tool allowlist
- **Anthropic Managed Agents** [anthropic.com/engineering]: Brain+Hands+Session (append-only)
- **Copilot JIT Memory** [github.blog]: Shared memory with validity_check on read
- **Codified Context** [arxiv 2602.20478]: Hot/Cold split, 67% fewer tokens with isolation

**Actionable:** Add `WorkingSetIsolation` enum: Shared/Owned/ReadOnly. Owned = sub-agent has private WorkingSet, merges back. ReadOnly = reads parent, can't modify.

---

## Implementation Priority

| P | Gap | Change | Lines | Crate |
|---|---|---|---|---|
| P0 | Memory Typing | `MemoryKind` enum + integrate with EpisodeSummary | ~60 | theo-domain |
| P1 | Hypothesis Engine | confidence + evidence fields on Hypothesis | ~50 | theo-domain |
| P2 | Failure Learning | FailureFingerprint + ring buffer + auto-constraint | ~80 | theo-domain + agent-runtime |
| P3 | Causal Usefulness | CausalLink tracking in ContextMetrics | ~70 | theo-agent-runtime |
| P4 | Multi-Agent Isolation | WorkingSetIsolation enum on WorkingSet | ~40 | theo-domain |
