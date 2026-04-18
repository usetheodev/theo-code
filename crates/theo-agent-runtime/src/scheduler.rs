use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use theo_domain::identifiers::TaskId;
use theo_domain::priority::Priority;

use crate::event_bus::EventBus;

/// Configuration for the task scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Maximum number of concurrent agent runs. Default: 1 (single-agent mode).
    pub max_concurrent_runs: usize,
    /// Maximum number of concurrent tool calls. Default: 1.
    pub max_concurrent_tool_calls: usize,
    /// Fairness window in seconds for round-robin between sessions.
    pub fairness_window_secs: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_runs: 1,
            max_concurrent_tool_calls: 1,
            fairness_window_secs: 60,
        }
    }
}

/// A task entry in the scheduler's priority queue.
#[derive(Debug, Clone)]
struct ScheduledTask {
    task_id: TaskId,
    priority: Priority,
    #[allow(dead_code)] // Stored for fairness window analysis
    enqueued_at: Instant,
    sequence: u64,
}

// BinaryHeap is a max-heap, so we want higher priority first.
// For equal priority, we want FIFO (earlier enqueued_at first = lower sequence first).
impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence)) // lower sequence = earlier = higher priority
    }
}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ScheduledTask {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}

impl Eq for ScheduledTask {}

/// Task scheduler with priority queue and concurrency control.
///
/// Manages the queue of tasks waiting to execute. In this phase (08),
/// the scheduler manages the QUEUE only — actual execution via RunEngine
/// is integrated in Phase 12.
pub struct Scheduler {
    config: SchedulerConfig,
    queue: Mutex<BinaryHeap<ScheduledTask>>,
    active: Mutex<Vec<TaskId>>,
    sequence_counter: Mutex<u64>,
    semaphore: Arc<tokio::sync::Semaphore>,
    _event_bus: Arc<EventBus>,
}

impl Scheduler {
    pub fn new(config: SchedulerConfig, event_bus: Arc<EventBus>) -> Self {
        let permits = config.max_concurrent_runs;
        Self {
            config,
            queue: Mutex::new(BinaryHeap::new()),
            active: Mutex::new(Vec::new()),
            sequence_counter: Mutex::new(0),
            semaphore: Arc::new(tokio::sync::Semaphore::new(permits)),
            _event_bus: event_bus,
        }
    }

    /// Submits a task to the scheduler queue with a given priority.
    pub fn submit(&self, task_id: TaskId, priority: Priority) {
        let mut seq = self.sequence_counter.lock().expect("seq lock poisoned");
        let sequence = *seq;
        *seq += 1;

        self.queue
            .lock()
            .expect("queue lock poisoned")
            .push(ScheduledTask {
                task_id,
                priority,
                enqueued_at: Instant::now(),
                sequence,
            });
    }

    /// Pops the highest-priority task from the queue.
    ///
    /// Returns None if the queue is empty.
    /// Moves the task to the active set.
    pub fn run_next(&self) -> Option<TaskId> {
        let task = self.queue.lock().expect("queue lock poisoned").pop()?;
        self.active
            .lock()
            .expect("active lock poisoned")
            .push(task.task_id.clone());
        Some(task.task_id)
    }

    /// Returns the number of currently active (running) tasks.
    pub fn active_count(&self) -> usize {
        self.active.lock().expect("active lock poisoned").len()
    }

    /// Returns the number of tasks waiting in the queue.
    pub fn queue_depth(&self) -> usize {
        self.queue.lock().expect("queue lock poisoned").len()
    }

    /// Cancels a task by removing it from the queue.
    ///
    /// Returns true if the task was found and removed, false otherwise.
    /// Does not cancel already-active tasks (that's the RunEngine's job).
    pub fn cancel(&self, task_id: &TaskId) -> bool {
        let mut queue = self.queue.lock().expect("queue lock poisoned");
        let before = queue.len();
        let items: Vec<_> = queue.drain().filter(|t| t.task_id != *task_id).collect();
        let removed = before > items.len();
        for item in items {
            queue.push(item);
        }
        removed
    }

    /// Marks a task as no longer active (completed/failed/cancelled).
    pub fn mark_completed(&self, task_id: &TaskId) {
        let mut active = self.active.lock().expect("active lock poisoned");
        active.retain(|id| id != task_id);
    }

    /// Drains the queue, returning all waiting task IDs.
    pub fn drain(&self) -> Vec<TaskId> {
        let mut queue = self.queue.lock().expect("queue lock poisoned");
        let tasks: Vec<TaskId> = queue.drain().map(|t| t.task_id).collect();
        tasks
    }

    /// Returns a reference to the concurrency semaphore.
    pub fn semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.semaphore
    }

    /// Returns the scheduler configuration.
    pub fn config(&self) -> &SchedulerConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Scheduler {
        let bus = Arc::new(EventBus::new());
        Scheduler::new(SchedulerConfig::default(), bus)
    }

    #[test]
    fn single_task_runs_immediately() {
        let sched = setup();
        sched.submit(TaskId::new("t-1"), Priority::Normal);
        assert_eq!(sched.queue_depth(), 1);

        let task = sched.run_next().unwrap();
        assert_eq!(task.as_str(), "t-1");
        assert_eq!(sched.queue_depth(), 0);
        assert_eq!(sched.active_count(), 1);
    }

    #[test]
    fn priority_ordering_critical_before_low() {
        let sched = setup();
        sched.submit(TaskId::new("low"), Priority::Low);
        sched.submit(TaskId::new("critical"), Priority::Critical);
        sched.submit(TaskId::new("high"), Priority::High);

        assert_eq!(sched.run_next().unwrap().as_str(), "critical");
        assert_eq!(sched.run_next().unwrap().as_str(), "high");
        assert_eq!(sched.run_next().unwrap().as_str(), "low");
    }

    #[test]
    fn fifo_for_equal_priority() {
        let sched = setup();
        sched.submit(TaskId::new("first"), Priority::Normal);
        sched.submit(TaskId::new("second"), Priority::Normal);
        sched.submit(TaskId::new("third"), Priority::Normal);

        assert_eq!(sched.run_next().unwrap().as_str(), "first");
        assert_eq!(sched.run_next().unwrap().as_str(), "second");
        assert_eq!(sched.run_next().unwrap().as_str(), "third");
    }

    #[test]
    fn active_count_and_mark_completed() {
        let sched = setup();
        sched.submit(TaskId::new("t-1"), Priority::Normal);
        assert_eq!(sched.active_count(), 0);

        let task = sched.run_next().unwrap();
        assert_eq!(sched.active_count(), 1);

        sched.mark_completed(&task);
        assert_eq!(sched.active_count(), 0);
    }

    #[test]
    fn queue_depth_correct() {
        let sched = setup();
        assert_eq!(sched.queue_depth(), 0);
        sched.submit(TaskId::new("a"), Priority::Normal);
        sched.submit(TaskId::new("b"), Priority::Normal);
        sched.submit(TaskId::new("c"), Priority::Normal);
        assert_eq!(sched.queue_depth(), 3);

        sched.run_next();
        assert_eq!(sched.queue_depth(), 2);
    }

    #[test]
    fn cancel_removes_from_queue() {
        let sched = setup();
        sched.submit(TaskId::new("keep"), Priority::Normal);
        sched.submit(TaskId::new("remove"), Priority::Normal);

        assert!(sched.cancel(&TaskId::new("remove")));
        assert_eq!(sched.queue_depth(), 1);
        assert_eq!(sched.run_next().unwrap().as_str(), "keep");
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let sched = setup();
        sched.submit(TaskId::new("t-1"), Priority::Normal);
        assert!(!sched.cancel(&TaskId::new("nonexistent")));
        assert_eq!(sched.queue_depth(), 1);
    }

    #[test]
    fn drain_clears_queue() {
        let sched = setup();
        sched.submit(TaskId::new("a"), Priority::High);
        sched.submit(TaskId::new("b"), Priority::Low);
        sched.submit(TaskId::new("c"), Priority::Normal);

        let tasks = sched.drain();
        assert_eq!(tasks.len(), 3);
        assert_eq!(sched.queue_depth(), 0);
    }

    #[test]
    fn empty_scheduler_run_next_returns_none() {
        let sched = setup();
        assert!(sched.run_next().is_none());
    }

    #[test]
    fn semaphore_has_correct_permits() {
        let bus = Arc::new(EventBus::new());
        let config = SchedulerConfig {
            max_concurrent_runs: 3,
            ..Default::default()
        };
        let sched = Scheduler::new(config, bus);
        assert_eq!(sched.semaphore().available_permits(), 3);
    }

    #[test]
    fn mixed_priorities_with_fifo_tiebreaker() {
        let sched = setup();
        sched.submit(TaskId::new("norm-1"), Priority::Normal);
        sched.submit(TaskId::new("high-1"), Priority::High);
        sched.submit(TaskId::new("norm-2"), Priority::Normal);
        sched.submit(TaskId::new("high-2"), Priority::High);

        assert_eq!(sched.run_next().unwrap().as_str(), "high-1");
        assert_eq!(sched.run_next().unwrap().as_str(), "high-2");
        assert_eq!(sched.run_next().unwrap().as_str(), "norm-1");
        assert_eq!(sched.run_next().unwrap().as_str(), "norm-2");
    }
}
