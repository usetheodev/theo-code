//! `ComplexityClassifier` — heurística determinística rule-based para
//! escolher o tier (Cheap/Default/Strong) baseado em sinais da task.
//!
//! Phase 14 — Cost-Aware Routing. Justificativa heurística sobre
//! learned-router: arXiv 2604.14228 §4.2 documenta que rules atingem
//! 95% accuracy em tarefas de agent.
//!
//! Sinais ordenados por força de evidência:
//! 1. Spec source (`OnDemand` → sempre Cheap; S1 já restringe a read-only)
//! 2. Prior failure count (>=2 → Strong; "throw money at it" pattern Aider)
//! 3. Task type (Planning/Analysis → Strong; Retrieval → Cheap; etc.)
//! 4. Token volume + tool count (fallback Generic)

use theo_domain::agent_spec::AgentSpecSource;
use theo_domain::routing::ComplexityTier;

/// Sinais extraídos da task + contexto do agent.
#[derive(Debug, Clone, Default)]
pub struct ComplexitySignals {
    pub system_prompt_tokens: u32,
    pub objective_tokens: u32,
    pub tool_count: u32,
    pub source: Option<AgentSpecSource>,
    pub task_type: TaskType,
    pub prior_failure_count: u32,
}

/// Categoria semântica da task. Detectada via keyword matching (deterministic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskType {
    /// "read", "list", "show", "explain" → barato
    Retrieval,
    /// "edit", "write", "implement", "refactor" → médio
    Implementation,
    /// "review", "audit", "analyze deeply" → caro (requer raciocínio)
    Analysis,
    /// "plan", "architect", "design" → caro (lead model)
    Planning,
    /// Fallback quando keywords não casam
    #[default]
    Generic,
}

pub struct ComplexityClassifier;

impl ComplexityClassifier {
    /// Heurística determinística (rules-based). Não usa LLM.
    pub fn classify(signals: &ComplexitySignals) -> ComplexityTier {
        // 1. On-demand é sempre Cheap (S1: read-only, max_iter=10)
        if signals.source == Some(AgentSpecSource::OnDemand) {
            return ComplexityTier::Cheap;
        }
        // 2. Prior failure → escala para Strong (try harder)
        if signals.prior_failure_count >= 2 {
            return ComplexityTier::Strong;
        }
        // 3. Task type é o sinal mais forte
        match signals.task_type {
            TaskType::Planning | TaskType::Analysis => ComplexityTier::Strong,
            TaskType::Retrieval => ComplexityTier::Cheap,
            TaskType::Implementation => {
                // Implementation com poucos tools + pequeno → Cheap
                if signals.tool_count <= 5 && signals.objective_tokens < 100 {
                    ComplexityTier::Cheap
                } else {
                    ComplexityTier::Default
                }
            }
            TaskType::Generic => {
                let total = signals.system_prompt_tokens + signals.objective_tokens;
                if total < 500 {
                    ComplexityTier::Cheap
                } else if total < 2000 {
                    ComplexityTier::Default
                } else {
                    ComplexityTier::Strong
                }
            }
        }
    }

    /// Detecta task_type a partir do objective via keyword matching.
    /// Determinístico; ordem de prioridade explícita: Planning > Analysis >
    /// Implementation > Retrieval > Generic. Keywords em English (objective
    /// é sempre em inglês na prática).
    pub fn detect_task_type(objective: &str) -> TaskType {
        let lower = objective.to_lowercase();
        if has_any_keyword(&lower, &["plan", "architect", "design system", "roadmap"]) {
            TaskType::Planning
        } else if has_any_keyword(&lower, &["review", "audit", "analyze", "security analysis"]) {
            TaskType::Analysis
        } else if has_any_keyword(
            &lower,
            &["implement", "edit", "write", "refactor", "fix bug", "create"],
        ) {
            TaskType::Implementation
        } else if has_any_keyword(
            &lower,
            &["read", "list", "show", "explain", "describe", "find"],
        ) {
            TaskType::Retrieval
        } else {
            TaskType::Generic
        }
    }
}

fn has_any_keyword(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals_for_type(task_type: TaskType) -> ComplexitySignals {
        ComplexitySignals {
            task_type,
            ..Default::default()
        }
    }

    // ── classify() ──

    #[test]
    fn complexity_classifier_on_demand_is_always_cheap() {
        let s = ComplexitySignals {
            source: Some(AgentSpecSource::OnDemand),
            // Even with strong-task indicators, on_demand wins
            task_type: TaskType::Planning,
            prior_failure_count: 5,
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Cheap);
    }

    #[test]
    fn complexity_classifier_prior_failure_2_escalates_to_strong() {
        let s = ComplexitySignals {
            prior_failure_count: 2,
            task_type: TaskType::Retrieval, // would be Cheap normally
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Strong);
    }

    #[test]
    fn complexity_classifier_prior_failure_1_does_not_escalate() {
        let s = ComplexitySignals {
            prior_failure_count: 1,
            task_type: TaskType::Retrieval,
            ..Default::default()
        };
        // Single failure stays Cheap (Retrieval)
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Cheap);
    }

    #[test]
    fn complexity_classifier_planning_is_strong() {
        assert_eq!(
            ComplexityClassifier::classify(&signals_for_type(TaskType::Planning)),
            ComplexityTier::Strong
        );
    }

    #[test]
    fn complexity_classifier_analysis_is_strong() {
        assert_eq!(
            ComplexityClassifier::classify(&signals_for_type(TaskType::Analysis)),
            ComplexityTier::Strong
        );
    }

    #[test]
    fn complexity_classifier_retrieval_is_cheap() {
        assert_eq!(
            ComplexityClassifier::classify(&signals_for_type(TaskType::Retrieval)),
            ComplexityTier::Cheap
        );
    }

    #[test]
    fn complexity_classifier_implementation_small_is_cheap() {
        let s = ComplexitySignals {
            task_type: TaskType::Implementation,
            tool_count: 3,
            objective_tokens: 50,
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Cheap);
    }

    #[test]
    fn complexity_classifier_implementation_large_is_default() {
        let s = ComplexitySignals {
            task_type: TaskType::Implementation,
            tool_count: 10,
            objective_tokens: 500,
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Default);
    }

    #[test]
    fn complexity_classifier_generic_under_500_tokens_is_cheap() {
        let s = ComplexitySignals {
            task_type: TaskType::Generic,
            system_prompt_tokens: 100,
            objective_tokens: 200,
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Cheap);
    }

    #[test]
    fn complexity_classifier_generic_500_to_2k_is_default() {
        let s = ComplexitySignals {
            task_type: TaskType::Generic,
            system_prompt_tokens: 800,
            objective_tokens: 400,
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Default);
    }

    #[test]
    fn complexity_classifier_generic_over_2k_tokens_is_strong() {
        let s = ComplexitySignals {
            task_type: TaskType::Generic,
            system_prompt_tokens: 1500,
            objective_tokens: 700,
            ..Default::default()
        };
        assert_eq!(ComplexityClassifier::classify(&s), ComplexityTier::Strong);
    }

    // ── detect_task_type() ──

    #[test]
    fn detect_task_type_planning_keywords() {
        assert_eq!(
            ComplexityClassifier::detect_task_type("plan the auth refactor"),
            TaskType::Planning
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("design system for ratelimit"),
            TaskType::Planning
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("draft the roadmap"),
            TaskType::Planning
        );
    }

    #[test]
    fn detect_task_type_analysis_keywords() {
        assert_eq!(
            ComplexityClassifier::detect_task_type("review the diff"),
            TaskType::Analysis
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("audit the auth module"),
            TaskType::Analysis
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("analyze performance hot path"),
            TaskType::Analysis
        );
    }

    #[test]
    fn detect_task_type_implementation_keywords() {
        assert_eq!(
            ComplexityClassifier::detect_task_type("implement caching layer"),
            TaskType::Implementation
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("edit the config file"),
            TaskType::Implementation
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("refactor the parser"),
            TaskType::Implementation
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("fix bug in retry logic"),
            TaskType::Implementation
        );
    }

    #[test]
    fn detect_task_type_retrieval_keywords() {
        assert_eq!(
            ComplexityClassifier::detect_task_type("read Cargo.toml"),
            TaskType::Retrieval
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("list all tests"),
            TaskType::Retrieval
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("explain how parser works"),
            TaskType::Retrieval
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("find the import of foo"),
            TaskType::Retrieval
        );
    }

    #[test]
    fn detect_task_type_generic_fallback() {
        assert_eq!(
            ComplexityClassifier::detect_task_type("hello world"),
            TaskType::Generic
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type(""),
            TaskType::Generic
        );
    }

    #[test]
    fn detect_task_type_priority_planning_over_implementation() {
        // "plan to implement" → Planning (priority order)
        assert_eq!(
            ComplexityClassifier::detect_task_type("plan to implement OAuth"),
            TaskType::Planning
        );
    }

    #[test]
    fn detect_task_type_case_insensitive() {
        assert_eq!(
            ComplexityClassifier::detect_task_type("AUDIT THE SECURITY"),
            TaskType::Analysis
        );
        assert_eq!(
            ComplexityClassifier::detect_task_type("Implement Foo"),
            TaskType::Implementation
        );
    }

    #[test]
    fn task_type_default_is_generic() {
        assert_eq!(TaskType::default(), TaskType::Generic);
    }
}
