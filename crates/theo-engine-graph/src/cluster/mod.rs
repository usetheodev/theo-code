//! Community detection (T4.2 split, D5).
//!
//! Sub-modules:
//!   - types.rs        — Community, ClusterResult
//!   - helpers.rs      — build_weight_map, degree, modularity
//!   - louvain.rs      — louvain_phase1, louvain_on_nodes, detect_communities
//!   - leiden.rs       — refine_partition, connected_components_of
//!   - lpa.rs          — lpa_seeded, leiden_communities
//!   - subdivide.rs    — subdivide_community, detect_file_communities
//!   - hierarchical.rs — two-level domain+module clustering
//!   - naming.rs       — community naming heuristics

mod helpers;
mod hierarchical;
mod leiden;
mod louvain;
mod lpa;
mod naming;
mod subdivide;
mod types;

pub use hierarchical::*;
pub use louvain::*;
pub use lpa::*;
pub use subdivide::*;
pub use types::*;
