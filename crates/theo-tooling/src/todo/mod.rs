//! Task management tools — Claude Code incremental pattern.
//!
//! Two tools: `task_create` (adds a task) and `task_update` (changes status by ID).
//! Tasks are never lost — updates are by position ID, not replace-all.

use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Completed => write!(f, "completed"),
            TodoStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: usize,
    pub content: String,
    pub status: TodoStatus,
}

// ---------------------------------------------------------------------------
// TodoList — shared, append-only, update-by-ID
// ---------------------------------------------------------------------------

pub struct TodoList {
    items: Mutex<Vec<TodoItem>>,
}

impl TodoList {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }

    /// Add a task. Returns the assigned ID (1-based).
    pub fn create(&self, content: String) -> usize {
        let mut items = self.items.lock().expect("todo lock poisoned");
        let id = items.len() + 1;
        items.push(TodoItem {
            id,
            content,
            status: TodoStatus::Pending,
        });
        id
    }

    /// Update status of a task by ID. Returns Ok if found, Err if not.
    pub fn update(&self, id: usize, status: TodoStatus) -> Result<(), String> {
        let mut items = self.items.lock().expect("todo lock poisoned");
        if let Some(item) = items.iter_mut().find(|t| t.id == id) {
            item.status = status;
            Ok(())
        } else {
            Err(format!(
                "task {} not found (have {} tasks)",
                id,
                items.len()
            ))
        }
    }

    pub fn list(&self) -> Vec<TodoItem> {
        self.items.lock().expect("todo lock poisoned").clone()
    }

    pub fn total(&self) -> usize {
        self.items.lock().expect("todo lock poisoned").len()
    }

    /// Format the full task list for display.
    pub fn format_list(&self) -> String {
        let items = self.items.lock().expect("todo lock poisoned");
        if items.is_empty() {
            return String::new();
        }
        let total = items.len();
        items
            .iter()
            .map(|t| {
                let icon = match t.status {
                    TodoStatus::Completed => "✅",
                    TodoStatus::InProgress => "🔄",
                    TodoStatus::Pending => "⬜",
                    TodoStatus::Cancelled => "❌",
                };
                format!("[{}/{}] {} {}", t.id, total, icon, t.content)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for TodoList {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// TaskCreateTool
// ---------------------------------------------------------------------------

pub struct TaskCreateTool;
impl TaskCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn id(&self) -> &str {
        "task_create"
    }

    fn description(&self) -> &str {
        "Create a new task to track progress. Returns the task ID. Use for any work with 3+ steps. Create all tasks FIRST, then start working."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "content".to_string(),
                param_type: "string".to_string(),
                description: "Description of the task".to_string(),
                required: true,
            }],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'content'".into()))?;

        // We can't access shared state from a stateless tool, so we return
        // the task info and let the RunEngine/ToolCallManager track it.
        // The output metadata carries the task info for the renderer.
        Ok(ToolOutput {
            title: format!("Task created: {}", content),
            output: format!("Created task: {content}"),
            metadata: serde_json::json!({
                "type": "task_create",
                "content": content,
            }),
            attachments: None,
        })
    }
}

// ---------------------------------------------------------------------------
// TaskUpdateTool
// ---------------------------------------------------------------------------

pub struct TaskUpdateTool;
impl TaskUpdateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn id(&self) -> &str {
        "task_update"
    }

    fn description(&self) -> &str {
        "Update the status of a task by ID. Mark as 'in_progress' before starting work, 'completed' immediately after finishing. Only ONE task should be in_progress at a time."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "id".to_string(),
                    param_type: "integer".to_string(),
                    description: "Task ID (from task_create)".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "status".to_string(),
                    param_type: "string".to_string(),
                    description: "New status: 'in_progress', 'completed', or 'cancelled'"
                        .to_string(),
                    required: true,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let id = args
            .get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'id' (integer)".into()))?
            as usize;

        let status_str = args
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'status'".into()))?;

        let status = match status_str {
            "pending" => TodoStatus::Pending,
            "in_progress" => TodoStatus::InProgress,
            "completed" => TodoStatus::Completed,
            "cancelled" => TodoStatus::Cancelled,
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "Invalid status: '{other}'. Use: pending, in_progress, completed, cancelled"
                )));
            }
        };

        let icon = match status {
            TodoStatus::Completed => "✅",
            TodoStatus::InProgress => "🔄",
            TodoStatus::Pending => "⬜",
            TodoStatus::Cancelled => "❌",
        };

        Ok(ToolOutput {
            title: format!("Task {id}: {icon} {status_str}"),
            output: format!("Task {id} → {status_str}"),
            metadata: serde_json::json!({
                "type": "task_update",
                "id": id,
                "status": status_str,
            }),
            attachments: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use std::sync::Arc;

    #[test]
    fn todo_status_serde_roundtrip() {
        for s in &[
            TodoStatus::Pending,
            TodoStatus::InProgress,
            TodoStatus::Completed,
            TodoStatus::Cancelled,
        ] {
            let json = serde_json::to_string(s).unwrap();
            let back: TodoStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, back);
        }
    }

    #[test]
    fn todo_list_create_increments_id() {
        let list = TodoList::new();
        let id1 = list.create("first".into());
        let id2 = list.create("second".into());
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(list.total(), 2);
    }

    #[test]
    fn todo_list_update_by_id() {
        let list = TodoList::new();
        list.create("task".into());
        assert!(list.update(1, TodoStatus::InProgress).is_ok());
        assert_eq!(list.list()[0].status, TodoStatus::InProgress);
        assert!(list.update(1, TodoStatus::Completed).is_ok());
        assert_eq!(list.list()[0].status, TodoStatus::Completed);
    }

    #[test]
    fn todo_list_update_nonexistent_returns_err() {
        let list = TodoList::new();
        assert!(list.update(99, TodoStatus::Completed).is_err());
    }

    #[test]
    fn todo_list_never_loses_tasks() {
        let list = TodoList::new();
        list.create("a".into());
        list.create("b".into());
        list.create("c".into());
        list.update(2, TodoStatus::Completed).unwrap();
        // All 3 still present
        assert_eq!(list.total(), 3);
        assert_eq!(list.list()[1].status, TodoStatus::Completed);
        assert_eq!(list.list()[0].status, TodoStatus::Pending);
        assert_eq!(list.list()[2].status, TodoStatus::Pending);
    }

    #[test]
    fn todo_list_format() {
        let list = TodoList::new();
        list.create("Read files".into());
        list.create("Write code".into());
        list.update(1, TodoStatus::Completed).unwrap();
        list.update(2, TodoStatus::InProgress).unwrap();
        let fmt = list.format_list();
        assert!(fmt.contains("✅ Read files"));
        assert!(fmt.contains("🔄 Write code"));
    }

    #[test]
    fn todo_list_thread_safe() {
        let list = Arc::new(TodoList::new());
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let l = list.clone();
                std::thread::spawn(move || {
                    l.create(format!("task {i}"));
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(list.total(), 10); // All 10 created, none lost
    }

    #[tokio::test]
    async fn task_create_tool_works() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let tool = TaskCreateTool::new();
        let result = tool
            .execute(
                serde_json::json!({"content": "Read project structure"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert!(result.output.contains("Read project structure"));
        assert_eq!(result.metadata["type"], "task_create");
    }

    #[tokio::test]
    async fn task_update_tool_works() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let tool = TaskUpdateTool::new();
        let result = tool
            .execute(
                serde_json::json!({"id": 1, "status": "completed"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert!(result.output.contains("completed"));
        assert_eq!(result.metadata["type"], "task_update");
    }

    #[tokio::test]
    async fn task_update_invalid_status_returns_error() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let tool = TaskUpdateTool::new();
        let result = tool
            .execute(
                serde_json::json!({"id": 1, "status": "invalid"}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn task_create_id() {
        assert_eq!(TaskCreateTool::new().id(), "task_create");
    }

    #[test]
    fn task_update_id() {
        assert_eq!(TaskUpdateTool::new().id(), "task_update");
    }
}
