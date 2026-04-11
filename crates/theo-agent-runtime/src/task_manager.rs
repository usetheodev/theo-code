use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use theo_domain::error::TransitionError;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::TaskId;
use theo_domain::session::SessionId;
use theo_domain::task::{AgentType, Artifact, Task, TaskState};

use crate::event_bus::EventBus;

/// Manages the lifecycle of Tasks, enforcing invariants and publishing events.
///
/// - **Invariant 1**: Every task has task_id, session_id, state, created_at.
/// - **Invariant 4**: Terminal states (Completed, Failed, Cancelled) reject all transitions.
/// - **Invariant 5**: Every state transition generates a DomainEvent.
///
/// Thread-safe via internal Mutex.
pub struct TaskManager {
    tasks: Mutex<HashMap<TaskId, Task>>,
    event_bus: Arc<EventBus>,
}

impl TaskManager {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            event_bus,
        }
    }

    /// Creates a new task with all required fields (Invariant 1).
    ///
    /// Publishes `DomainEvent::TaskCreated` (Invariant 5).
    pub fn create_task(
        &self,
        session_id: SessionId,
        agent_type: AgentType,
        objective: String,
    ) -> TaskId {
        let task_id = TaskId::generate();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_millis() as u64;

        let task = Task {
            task_id: task_id.clone(),
            session_id,
            state: TaskState::Pending,
            agent_type,
            objective: objective.clone(),
            artifacts: Vec::new(),
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        self.tasks
            .lock()
            .expect("tasks lock poisoned")
            .insert(task_id.clone(), task);

        // Invariant 5: publish TaskCreated event
        self.event_bus.publish(DomainEvent::new(
            EventType::TaskCreated,
            task_id.as_str(),
            serde_json::json!({
                "objective": objective,
            }),
        ));

        task_id
    }

    /// Transitions a task to a new state (Invariants 4 + 5).
    ///
    /// - Invariant 4: Terminal states reject all transitions (enforced by TaskState).
    /// - Invariant 5: Publishes `DomainEvent::TaskStateChanged` with from/to payload.
    pub fn transition(&self, task_id: &TaskId, target: TaskState) -> Result<(), TaskManagerError> {
        let mut tasks = self.tasks.lock().expect("tasks lock poisoned");
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskManagerError::TaskNotFound(task_id.as_str().to_string()))?;

        let from = task.state;
        task.transition(target)?;

        // Invariant 5: publish TaskStateChanged event
        self.event_bus.publish(DomainEvent::new(
            EventType::TaskStateChanged,
            task_id.as_str(),
            serde_json::json!({
                "from": format!("{:?}", from),
                "to": format!("{:?}", target),
            }),
        ));

        Ok(())
    }

    /// Returns a clone of the task, or None if not found.
    pub fn get(&self, task_id: &TaskId) -> Option<Task> {
        self.tasks
            .lock()
            .expect("tasks lock poisoned")
            .get(task_id)
            .cloned()
    }

    /// Returns all tasks for a given session.
    pub fn tasks_by_session(&self, session_id: &SessionId) -> Vec<Task> {
        self.tasks
            .lock()
            .expect("tasks lock poisoned")
            .values()
            .filter(|t| t.session_id == *session_id)
            .cloned()
            .collect()
    }

    /// Returns all non-terminal tasks.
    pub fn active_tasks(&self) -> Vec<Task> {
        self.tasks
            .lock()
            .expect("tasks lock poisoned")
            .values()
            .filter(|t| !t.state.is_terminal())
            .cloned()
            .collect()
    }

    /// Adds an artifact to a task.
    pub fn add_artifact(
        &self,
        task_id: &TaskId,
        artifact: Artifact,
    ) -> Result<(), TaskManagerError> {
        let mut tasks = self.tasks.lock().expect("tasks lock poisoned");
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskManagerError::TaskNotFound(task_id.as_str().to_string()))?;
        task.artifacts.push(artifact);
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskManagerError {
    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("transition error: {0}")]
    Transition(#[from] TransitionError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;

    fn setup() -> (TaskManager, Arc<CapturingListener>) {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());
        let manager = TaskManager::new(bus);
        (manager, listener)
    }

    // -----------------------------------------------------------------------
    // Invariant 1: create_task fields
    // -----------------------------------------------------------------------

    #[test]
    fn create_task_has_all_required_fields() {
        let (manager, _) = setup();
        let task_id =
            manager.create_task(SessionId::new("s-1"), AgentType::Coder, "fix bug".into());
        let task = manager.get(&task_id).expect("task must exist");
        assert_eq!(task.task_id, task_id);
        assert_eq!(task.session_id, SessionId::new("s-1"));
        assert_eq!(task.state, TaskState::Pending);
        assert!(task.created_at > 0);
        assert_eq!(task.updated_at, task.created_at);
        assert!(task.completed_at.is_none());
    }

    #[test]
    fn create_task_emits_task_created_event() {
        let (manager, listener) = setup();
        let task_id =
            manager.create_task(SessionId::new("s-1"), AgentType::Coder, "fix bug".into());
        let events = listener.captured();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::TaskCreated);
        assert_eq!(events[0].entity_id, task_id.as_str());
    }

    // -----------------------------------------------------------------------
    // Invariant 4: terminal states
    // -----------------------------------------------------------------------

    #[test]
    fn completed_to_running_returns_err() {
        let (manager, _) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();
        manager.transition(&id, TaskState::Running).unwrap();
        manager.transition(&id, TaskState::Completed).unwrap();

        let result = manager.transition(&id, TaskState::Running);
        assert!(result.is_err());
    }

    #[test]
    fn failed_rejects_transitions() {
        let (manager, _) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();
        manager.transition(&id, TaskState::Running).unwrap();
        manager.transition(&id, TaskState::Failed).unwrap();

        assert!(manager.transition(&id, TaskState::Running).is_err());
    }

    #[test]
    fn cancelled_rejects_transitions() {
        let (manager, _) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();
        manager.transition(&id, TaskState::Cancelled).unwrap();

        assert!(manager.transition(&id, TaskState::Running).is_err());
    }

    // -----------------------------------------------------------------------
    // Invariant 5: transitions emit events
    // -----------------------------------------------------------------------

    #[test]
    fn transition_emits_task_state_changed_with_payload() {
        let (manager, listener) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();

        let events = listener.captured();
        // events[0] = TaskCreated, events[1] = TaskStateChanged
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event_type, EventType::TaskStateChanged);
        assert_eq!(events[1].entity_id, id.as_str());

        let payload = &events[1].payload;
        assert_eq!(payload["from"].as_str().unwrap(), "Pending");
        assert_eq!(payload["to"].as_str().unwrap(), "Ready");
    }

    #[test]
    fn failed_transition_does_not_emit_event() {
        let (manager, listener) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        let _ = manager.transition(&id, TaskState::Completed); // invalid: Pending→Completed

        let events = listener.captured();
        // Only TaskCreated, no TaskStateChanged
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::TaskCreated);
    }

    // -----------------------------------------------------------------------
    // Happy paths
    // -----------------------------------------------------------------------

    #[test]
    fn happy_path_pending_to_completed() {
        let (manager, _) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();
        manager.transition(&id, TaskState::Running).unwrap();
        manager.transition(&id, TaskState::Completed).unwrap();

        let task = manager.get(&id).unwrap();
        assert_eq!(task.state, TaskState::Completed);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn path_running_to_failed() {
        let (manager, _) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();
        manager.transition(&id, TaskState::Running).unwrap();
        manager.transition(&id, TaskState::Failed).unwrap();

        let task = manager.get(&id).unwrap();
        assert_eq!(task.state, TaskState::Failed);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn path_with_waiting_tool() {
        let (manager, listener) = setup();
        let id = manager.create_task(SessionId::new("s"), AgentType::Coder, "t".into());
        manager.transition(&id, TaskState::Ready).unwrap();
        manager.transition(&id, TaskState::Running).unwrap();
        manager.transition(&id, TaskState::WaitingTool).unwrap();
        manager.transition(&id, TaskState::Running).unwrap();
        manager.transition(&id, TaskState::Completed).unwrap();

        let task = manager.get(&id).unwrap();
        assert_eq!(task.state, TaskState::Completed);
        // 1 TaskCreated + 5 TaskStateChanged = 6 events
        assert_eq!(listener.captured().len(), 6);
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    #[test]
    fn tasks_by_session_filters_correctly() {
        let (manager, _) = setup();
        manager.create_task(SessionId::new("s-1"), AgentType::Coder, "a".into());
        manager.create_task(SessionId::new("s-1"), AgentType::Reviewer, "b".into());
        manager.create_task(SessionId::new("s-2"), AgentType::Coder, "c".into());

        let s1_tasks = manager.tasks_by_session(&SessionId::new("s-1"));
        assert_eq!(s1_tasks.len(), 2);

        let s2_tasks = manager.tasks_by_session(&SessionId::new("s-2"));
        assert_eq!(s2_tasks.len(), 1);
    }

    #[test]
    fn active_tasks_excludes_all_three_terminal_states() {
        let (manager, _) = setup();
        let id1 = manager.create_task(SessionId::new("s"), AgentType::Coder, "a".into());
        let id2 = manager.create_task(SessionId::new("s"), AgentType::Coder, "b".into());
        let id3 = manager.create_task(SessionId::new("s"), AgentType::Coder, "c".into());
        let id4 = manager.create_task(SessionId::new("s"), AgentType::Coder, "d".into());

        // Move id1 to Completed
        manager.transition(&id1, TaskState::Ready).unwrap();
        manager.transition(&id1, TaskState::Running).unwrap();
        manager.transition(&id1, TaskState::Completed).unwrap();

        // Move id2 to Failed
        manager.transition(&id2, TaskState::Ready).unwrap();
        manager.transition(&id2, TaskState::Running).unwrap();
        manager.transition(&id2, TaskState::Failed).unwrap();

        // Move id3 to Cancelled
        manager.transition(&id3, TaskState::Cancelled).unwrap();

        // id4 stays Pending (active)
        let active = manager.active_tasks();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].task_id, id4);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let (manager, _) = setup();
        assert!(manager.get(&TaskId::new("nonexistent")).is_none());
    }

    #[test]
    fn transition_nonexistent_returns_err() {
        let (manager, _) = setup();
        let result = manager.transition(&TaskId::new("nonexistent"), TaskState::Ready);
        assert!(matches!(result, Err(TaskManagerError::TaskNotFound(_))));
    }

    // -----------------------------------------------------------------------
    // Thread safety
    // -----------------------------------------------------------------------

    #[test]
    fn concurrent_transitions_on_different_tasks() {
        let bus = Arc::new(EventBus::new());
        let manager = Arc::new(TaskManager::new(bus));

        let id1 = manager.create_task(SessionId::new("s"), AgentType::Coder, "a".into());
        let id2 = manager.create_task(SessionId::new("s"), AgentType::Coder, "b".into());

        let m1 = manager.clone();
        let id1c = id1.clone();
        let h1 = std::thread::spawn(move || {
            m1.transition(&id1c, TaskState::Ready).unwrap();
            m1.transition(&id1c, TaskState::Running).unwrap();
        });

        let m2 = manager.clone();
        let id2c = id2.clone();
        let h2 = std::thread::spawn(move || {
            m2.transition(&id2c, TaskState::Ready).unwrap();
            m2.transition(&id2c, TaskState::Running).unwrap();
        });

        h1.join().unwrap();
        h2.join().unwrap();

        assert_eq!(manager.get(&id1).unwrap().state, TaskState::Running);
        assert_eq!(manager.get(&id2).unwrap().state, TaskState::Running);
    }
}
