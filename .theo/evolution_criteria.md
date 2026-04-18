# SOTA Criteria: Context Manager Evolution

**Target subsystem:** theo-domain (types), theo-application (assembler), theo-agent-runtime (metrics)
**Reference bar:** OpenDev (Rust, same language), Pi-Mono (compaction), awesome-harness-engineering (2026 papers)

---

## Dimension-Specific Criteria

### Pattern Fidelity
- Memory typing follows MemGPT three-tier model (Core/Recall/Archival → Ephemeral/Episodic/Reusable/Canonical)
- Hypothesis confidence follows LATS-style evidence accumulation
- Failure learning follows OpenDev DoomLoopDetector fingerprint pattern
- Causal tracking follows CausalTrace append-only pattern
- Multi-agent isolation follows OpenDev SubAgentSpec IsolationMode pattern

### Architectural Fit
- New types in `theo-domain` (zero dependencies)
- Integration through existing traits and patterns
- No new crate dependencies
- Respects existing MemoryLifecycle (extends, doesn't replace)

### Completeness
- All new enums have `Default` impl
- Error cases handled with typed errors
- Edge cases: empty collections, zero evidence, missing agent_id

### Testability
- Each new type has unit tests (Arrange-Act-Assert)
- Edge case tests for transitions (hypothesis stale threshold, fingerprint recurrence)
- Integration with existing structural_hygiene tests

### Simplicity
- Enums over complex structs where possible
- No trait unless ≥2 implementations exist
- Max 200 lines per change cycle
- Each gap is a focused, independent change

---

## Convergence Criteria

Average ≥ 2.5 across all 5 dimensions with:
- Pattern Fidelity ≥ 2 (patterns identified and applied)
- Architectural Fit ≥ 2 (boundaries respected)
- Completeness ≥ 2 (core + edge cases)
- Testability ≥ 2 (happy path + edge cases tested)
- Simplicity ≥ 2 (clean, focused, no over-engineering)
