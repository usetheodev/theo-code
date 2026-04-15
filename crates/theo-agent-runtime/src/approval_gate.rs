//! Interactive approval gate — pauses tool execution for human approval.
//!
//! Implements the handshake protocol defined in ADR-004:
//! 1. Runtime publishes GovernanceDecisionPending
//! 2. Runtime awaits on oneshot channel
//! 3. TUI shows modal, user approves/rejects
//! 4. TUI resolves the decision via resolve()
//! 5. Runtime receives outcome and continues/aborts

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::oneshot;

use theo_domain::event::{DomainEvent, EventType};

use crate::event_bus::EventBus;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Risk level for a tool call — determines whether approval is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Request for approval of a tool execution.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub decision_id: String,
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub risk_level: RiskLevel,
}

/// Outcome of an approval request.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalOutcome {
    Approved,
    Rejected(String),
    Timeout,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Gate that can pause tool execution for interactive approval.
#[async_trait]
pub trait ApprovalGate: Send + Sync {
    /// Request approval for a tool call. Blocks until resolved or timeout.
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalOutcome;
}

// ---------------------------------------------------------------------------
// AutoApproveGate — for legacy CLI and tests
// ---------------------------------------------------------------------------

/// Approves all requests immediately. No user interaction.
pub struct AutoApproveGate;

#[async_trait]
impl ApprovalGate for AutoApproveGate {
    async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalOutcome {
        ApprovalOutcome::Approved
    }
}

// ---------------------------------------------------------------------------
// TuiApprovalGate — interactive with oneshot channels
// ---------------------------------------------------------------------------

/// Interactive approval gate that pauses for TUI modal approval.
///
/// Protocol:
/// 1. `request_approval()` publishes GovernanceDecisionPending, creates oneshot, waits
/// 2. TUI calls `resolve()` with decision_id and outcome
/// 3. `request_approval()` receives outcome via oneshot and returns
pub struct TuiApprovalGate {
    event_bus: Arc<EventBus>,
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalOutcome>>>,
    timeout: Duration,
    /// Minimum risk level that requires approval. Lower levels auto-approve.
    min_approval_risk: RiskLevel,
}

impl TuiApprovalGate {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            pending: Mutex::new(HashMap::new()),
            timeout: Duration::from_secs(300), // 5 minutes
            min_approval_risk: RiskLevel::Medium,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_min_risk(mut self, risk: RiskLevel) -> Self {
        self.min_approval_risk = risk;
        self
    }

    /// Resolve a pending decision (called by TUI when user approves/rejects).
    pub fn resolve(&self, decision_id: &str, outcome: ApprovalOutcome) {
        let sender = self.pending
            .lock()
            .expect("pending lock")
            .remove(decision_id);

        if let Some(tx) = sender {
            let _ = tx.send(outcome.clone());
        }

        // Publish resolved event
        self.event_bus.publish(DomainEvent::new(
            EventType::GovernanceDecisionResolved,
            decision_id,
            serde_json::json!({
                "decision_id": decision_id,
                "outcome": match &outcome {
                    ApprovalOutcome::Approved => "approved",
                    ApprovalOutcome::Rejected(_) => "rejected",
                    ApprovalOutcome::Timeout => "timeout",
                },
            }),
        ));
    }

    fn risk_requires_approval(&self, risk: RiskLevel) -> bool {
        let risk_ord = match risk {
            RiskLevel::Low => 0,
            RiskLevel::Medium => 1,
            RiskLevel::High => 2,
            RiskLevel::Critical => 3,
        };
        let min_ord = match self.min_approval_risk {
            RiskLevel::Low => 0,
            RiskLevel::Medium => 1,
            RiskLevel::High => 2,
            RiskLevel::Critical => 3,
        };
        risk_ord >= min_ord
    }
}

#[async_trait]
impl ApprovalGate for TuiApprovalGate {
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalOutcome {
        // Auto-approve low-risk tools
        if !self.risk_requires_approval(request.risk_level) {
            return ApprovalOutcome::Approved;
        }

        let decision_id = request.decision_id.clone();

        // Create oneshot channel
        let (tx, rx) = oneshot::channel();

        // Register pending decision
        self.pending
            .lock()
            .expect("pending lock")
            .insert(decision_id.clone(), tx);

        // Publish pending event for TUI to display
        self.event_bus.publish(DomainEvent::new(
            EventType::GovernanceDecisionPending,
            &decision_id,
            serde_json::json!({
                "decision_id": decision_id,
                "tool_name": request.tool_name,
                "risk_level": format!("{:?}", request.risk_level),
                "args_preview": truncate_args(&request.tool_args),
            }),
        ));

        // Wait for resolution or timeout
        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => {
                // Sender dropped without sending — treat as rejection
                ApprovalOutcome::Rejected("Decision cancelled".to_string())
            }
            Err(_) => {
                // Timeout
                self.pending.lock().expect("pending lock").remove(&decision_id);
                self.event_bus.publish(DomainEvent::new(
                    EventType::GovernanceDecisionResolved,
                    &decision_id,
                    serde_json::json!({
                        "decision_id": decision_id,
                        "outcome": "timeout",
                    }),
                ));
                ApprovalOutcome::Timeout
            }
        }
    }
}

fn truncate_args(args: &serde_json::Value) -> String {
    let s = args.to_string();
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s
    }
}

/// Determine risk level for a tool based on its name.
pub fn tool_risk_level(tool_name: &str) -> RiskLevel {
    match tool_name {
        "read" | "glob" | "grep" | "think" | "reflect" | "task_create" | "task_update" | "done" => RiskLevel::Low,
        "write" | "edit" | "apply_patch" | "multiedit" => RiskLevel::Medium,
        "bash" | "webfetch" | "websearch" | "shell" => RiskLevel::High,
        _ => RiskLevel::Medium,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auto_approve_gate_approves_all() {
        let gate = AutoApproveGate;
        let request = ApprovalRequest {
            decision_id: "d-1".to_string(),
            tool_name: "bash".to_string(),
            tool_args: serde_json::json!({"command": "ls"}),
            risk_level: RiskLevel::High,
        };
        assert_eq!(gate.request_approval(request).await, ApprovalOutcome::Approved);
    }

    #[tokio::test]
    async fn tui_approval_gate_approve_flow() {
        let bus = Arc::new(EventBus::new());
        let gate = Arc::new(TuiApprovalGate::new(bus.clone()).with_min_risk(RiskLevel::Low));

        let gate_clone = gate.clone();
        let handle = tokio::spawn(async move {
            let request = ApprovalRequest {
                decision_id: "d-1".to_string(),
                tool_name: "bash".to_string(),
                tool_args: serde_json::json!({}),
                risk_level: RiskLevel::High,
            };
            gate_clone.request_approval(request).await
        });

        // Give the request time to register
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Resolve from "TUI"
        gate.resolve("d-1", ApprovalOutcome::Approved);

        let outcome = handle.await.unwrap();
        assert_eq!(outcome, ApprovalOutcome::Approved);
    }

    #[tokio::test]
    async fn tui_approval_gate_reject_flow() {
        let bus = Arc::new(EventBus::new());
        let gate = Arc::new(TuiApprovalGate::new(bus.clone()).with_min_risk(RiskLevel::Low));

        let gate_clone = gate.clone();
        let handle = tokio::spawn(async move {
            let request = ApprovalRequest {
                decision_id: "d-2".to_string(),
                tool_name: "bash".to_string(),
                tool_args: serde_json::json!({}),
                risk_level: RiskLevel::High,
            };
            gate_clone.request_approval(request).await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        gate.resolve("d-2", ApprovalOutcome::Rejected("user said no".into()));

        let outcome = handle.await.unwrap();
        assert_eq!(outcome, ApprovalOutcome::Rejected("user said no".into()));
    }

    #[tokio::test]
    async fn tui_approval_gate_timeout() {
        let bus = Arc::new(EventBus::new());
        let gate = TuiApprovalGate::new(bus)
            .with_timeout(Duration::from_millis(50))
            .with_min_risk(RiskLevel::Low);

        let request = ApprovalRequest {
            decision_id: "d-3".to_string(),
            tool_name: "bash".to_string(),
            tool_args: serde_json::json!({}),
            risk_level: RiskLevel::High,
        };

        let outcome = gate.request_approval(request).await;
        assert_eq!(outcome, ApprovalOutcome::Timeout);
    }

    #[tokio::test]
    async fn low_risk_auto_approved() {
        let bus = Arc::new(EventBus::new());
        let gate = TuiApprovalGate::new(bus).with_min_risk(RiskLevel::Medium);

        let request = ApprovalRequest {
            decision_id: "d-4".to_string(),
            tool_name: "read".to_string(),
            tool_args: serde_json::json!({}),
            risk_level: RiskLevel::Low,
        };

        assert_eq!(gate.request_approval(request).await, ApprovalOutcome::Approved);
    }

    #[test]
    fn tool_risk_levels_correct() {
        assert_eq!(tool_risk_level("read"), RiskLevel::Low);
        assert_eq!(tool_risk_level("bash"), RiskLevel::High);
        assert_eq!(tool_risk_level("write"), RiskLevel::Medium);
        assert_eq!(tool_risk_level("unknown"), RiskLevel::Medium);
    }
}
