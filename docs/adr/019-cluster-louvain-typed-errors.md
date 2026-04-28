# ADR-019 — Cluster Louvain heuristics: typed errors instead of `.unwrap()`

**Status:** Accepted
**Date:** 2026-04-28
**Plan:** `docs/plans/code-hygiene-5x5-plan.md` (T1.1, Phase 1)

## Context

The Louvain / LPA community-detection heuristics in
`crates/theo-engine-graph/src/cluster/{subdivide,lpa}.rs` had 7 `.unwrap()`
sites covered by a regex allowlist entry (sunset 2026-08-31):

```
regex:crates/theo-engine-graph/**/*.rs@@\.unwrap\(\)@@2026-10-31@@Louvain
heuristic invariants — partial_cmp on non-NaN floats and HashMap::get on
label-set built from same iterated ids.
```

Two distinct invariants:

1. `labels.get(node_id).unwrap()` — every iterated node has a label, because
   the label set was built from the same node-id list.
2. `partial_cmp(wb).unwrap()` — edge weights are normalised, finite floats;
   `partial_cmp` should never return `None`.
3. `.max_by(...).map(...).unwrap()` — at least one neighbor exists when this
   branch is reached (guarded by `is_empty` check earlier).

Treating these as `.unwrap()` was a **Cat-B** bet (real production paths,
real invariants — see `docs/plans/cleanup-2026-04-28.md` CLEAN-B1) tracked
for revisit at sunset.

## Decision

Convert the 7 sites to **typed errors** via a new `ClusterError` enum
defined in `crates/theo-engine-graph/src/cluster/types.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    #[error("missing label for node `{0}`; partition state is internally inconsistent")]
    MissingLabel(String),

    #[error("max_by on neighbor weights returned None; expected at least one neighbor in this branch")]
    EmptyNeighbors,
}
```

Functions `subdivide_community` and `lpa_seeded` now return
`Result<T, ClusterError>`. Callers either propagate the error (internal
boundary) or **degrade gracefully** at the public Vec<Community> boundary
by falling back to directory-seeded labels with an `eprintln!` warning
instead of panicking.

For the `partial_cmp` inside `.max_by(|...| Ordering)` closure (where `?`
isn't available), we use `.partial_cmp(wb).unwrap_or(Ordering::Equal)` —
biases toward "first encountered" on the (unreachable) NaN case rather
than panicking.

## Rationale

Three reasons over keeping `.unwrap()` + allowlist:

1. **Honesty in error surface.** `Result<T, ClusterError>` documents the
   algorithmic contract in the type system. A reader doesn't need to grep
   the allowlist to learn "this function can fail because of an internal
   inconsistency".
2. **Graceful degradation at the boundary.** The public API
   (`hierarchical_cluster`, `detect_file_communities`) used to panic if a
   Louvain invariant violated. Now it logs + returns the directory-seeded
   partition. A retrieval pipeline degrading to "no semantic clustering"
   is much better than a process crash.
3. **Removes a regex allowlist entry.** Per the *code-hygiene-5x5* plan,
   the unwrap-allowlist should drain to ≤ 5 path entries. This one is
   gone now.

Alternatives considered:

- **Keep `.unwrap()` + path-specific allowlist:** rejected. Path-specific
  entries don't help anyone reading the code; the typed error does.
- **Use `.expect("invariant: ...")` everywhere:** rejected. ADR-021
  (Phase 2 of *code-hygiene-5x5*) is meant to codify legitimate
  `.expect()` patterns; using it here for an invariant we're *removing*
  defeats the purpose.
- **Refactor to use `Vec<usize>` indexed labels instead of HashMap:**
  rejected as scope creep. The HashMap is correct; the unwrap was the
  bug.

## Consequences

- **API change:** `lpa_seeded` and `subdivide_community` now return
  `Result<T, ClusterError>`. Internal callers updated; tests updated to
  `.expect("...should succeed on test fixture")`.
- **Behavior change:** `subdivide_with_lpa_seeded` now logs + degrades
  instead of panicking when LPA fails. The public `hierarchical_cluster`
  signature is unchanged.
- **Allowlist removed:** the cluster regex entry in
  `.claude/rules/unwrap-allowlist.txt` is now `# REMOVED`. The
  path-specific stale entry for the now-deleted `cluster.rs` is also
  cleaned up.
- **Future:** if `ClusterError::MissingLabel` ever fires in production,
  we have a typed signal to triage instead of a stack trace. The error
  carries the offending node id.

## Cross-references

- `docs/plans/cleanup-2026-04-28.md` — CLEAN-B1 (the original
  Cat-B classification).
- `docs/plans/god-files-2026-07-23-plan.md` — T4.2 (the cluster.rs
  module-dir split that inherited these unwraps).
- `docs/plans/code-hygiene-5x5-plan.md` — T1.1 (this ADR).
- `crates/theo-engine-graph/src/cluster/types.rs` — `ClusterError` definition.
- `crates/theo-engine-graph/src/cluster/{subdivide,lpa}.rs` — call sites.
- `crates/theo-engine-graph/src/cluster/hierarchical.rs` — graceful
  degradation boundary.
