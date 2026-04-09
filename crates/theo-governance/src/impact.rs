/// Impact analysis types re-exported from theo-domain.
///
/// The `ImpactReport` type lives in `theo_domain::graph_context`.
/// The `analyze_impact` implementation lives in `theo_application::use_cases::impact`.
///
/// This module preserves backward compatibility for downstream consumers
/// that import `theo_governance::impact::ImpactReport`.
pub use theo_domain::graph_context::ImpactReport;
