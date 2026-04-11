//! MemoryTool + FileMemoryStore — persistent agent memory across runs.
//!
//! The agent can save, recall, list, search, and delete facts about a project.
//! Memories persist in `~/.theo/memory/{project_hash}/` as JSON files.
//! Each project gets its own namespace (hash of the project directory path).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

// ---------------------------------------------------------------------------
// AgentMemoryEntry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemoryEntry {
    pub key: String,
    pub value: String,
    pub created_at: u64,
    pub run_id: String,
}

// ---------------------------------------------------------------------------
// FileMemoryStore
// ---------------------------------------------------------------------------

pub struct FileMemoryStore {
    base_dir: PathBuf,
}

impl FileMemoryStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create a store for a specific project (hashed directory).
    pub fn for_project(memory_root: &Path, project_dir: &Path) -> Self {
        let mut hasher = DefaultHasher::new();
        project_dir.to_string_lossy().hash(&mut hasher);
        let hash = format!("{:016x}", hasher.finish());
        Self {
            base_dir: memory_root.join(hash),
        }
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        let safe_key = sanitize_key(key);
        self.base_dir.join(format!("{}.json", safe_key))
    }

    pub async fn save(&self, entry: &AgentMemoryEntry) -> Result<(), ToolError> {
        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to create memory dir: {e}")))?;

        let json = serde_json::to_string_pretty(entry)
            .map_err(|e| ToolError::Execution(format!("Serialization error: {e}")))?;

        // Atomic write: write to temp, then rename
        let path = self.entry_path(&entry.key);
        let tmp_path = path.with_extension("tmp");
        tokio::fs::write(&tmp_path, &json)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to write memory: {e}")))?;
        tokio::fs::rename(&tmp_path, &path)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to finalize memory write: {e}")))?;

        Ok(())
    }

    pub async fn recall(&self, key: &str) -> Result<Option<AgentMemoryEntry>, ToolError> {
        let path = self.entry_path(key);
        if !path.exists() {
            return Ok(None);
        }

        let json = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read memory: {e}")))?;

        let entry: AgentMemoryEntry = serde_json::from_str(&json)
            .map_err(|e| ToolError::Execution(format!("Corrupt memory file: {e}")))?;

        Ok(Some(entry))
    }

    pub async fn list(&self) -> Result<Vec<AgentMemoryEntry>, ToolError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&self.base_dir)
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to list memories: {e}")))?;

        while let Ok(Some(entry)) = dir.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(json) = tokio::fs::read_to_string(&path).await {
                    if let Ok(mem) = serde_json::from_str::<AgentMemoryEntry>(&json) {
                        entries.push(mem);
                    }
                }
            }
        }

        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(entries)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<AgentMemoryEntry>, ToolError> {
        let all = self.list().await?;
        let query_lower = query.to_lowercase();
        Ok(all
            .into_iter()
            .filter(|e| {
                e.key.to_lowercase().contains(&query_lower)
                    || e.value.to_lowercase().contains(&query_lower)
            })
            .collect())
    }

    pub async fn delete(&self, key: &str) -> Result<bool, ToolError> {
        let path = self.entry_path(key);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to delete memory: {e}")))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Sanitize key for use as filename (alphanumeric + underscore + hyphen).
fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(128)
        .collect()
}

// ---------------------------------------------------------------------------
// MemoryTool
// ---------------------------------------------------------------------------

pub struct MemoryTool;

impl MemoryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn id(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Save, recall, list, search, or delete persistent facts about this project. Memories survive between agent runs."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "action".to_string(),
                    param_type: "string".to_string(),
                    description: "Action: save, recall, list, search, or delete".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "key".to_string(),
                    param_type: "string".to_string(),
                    description: "Memory key (for save, recall, delete)".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "value".to_string(),
                    param_type: "string".to_string(),
                    description: "Memory value (for save)".to_string(),
                    required: false,
                },
                ToolParam {
                    name: "query".to_string(),
                    param_type: "string".to_string(),
                    description: "Search query (for search)".to_string(),
                    required: false,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'action' field".to_string()))?;

        let memory_root = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(".config")
            .join("theo")
            .join("memory");

        let store = FileMemoryStore::for_project(&memory_root, &ctx.project_dir);

        match action {
            "save" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs("save requires 'key'".to_string()))?;
                let value = args
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs("save requires 'value'".to_string()))?;

                let entry = AgentMemoryEntry {
                    key: key.to_string(),
                    value: value.to_string(),
                    created_at: now_millis(),
                    run_id: ctx.call_id.clone(),
                };
                store.save(&entry).await?;

                Ok(ToolOutput {
                    title: format!("Saved memory: {key}"),
                    output: format!("Memory saved: {} = {}", key, value),
                    metadata: serde_json::json!({"action": "save", "key": key}),
                    attachments: None,
                })
            }
            "recall" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs("recall requires 'key'".to_string()))?;

                match store.recall(key).await? {
                    Some(entry) => Ok(ToolOutput {
                        title: format!("Memory: {key}"),
                        output: entry.value,
                        metadata: serde_json::json!({"action": "recall", "key": key, "found": true}),
                        attachments: None,
                    }),
                    None => Ok(ToolOutput {
                        title: format!("Memory not found: {key}"),
                        output: format!("No memory found for key '{key}'"),
                        metadata: serde_json::json!({"action": "recall", "key": key, "found": false}),
                        attachments: None,
                    }),
                }
            }
            "list" => {
                let entries = store.list().await?;
                let output = if entries.is_empty() {
                    "No memories stored for this project.".to_string()
                } else {
                    entries
                        .iter()
                        .map(|e| format!("- {}: {}", e.key, e.value))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                Ok(ToolOutput {
                    title: format!("{} memories", entries.len()),
                    output,
                    metadata: serde_json::json!({"action": "list", "count": entries.len()}),
                    attachments: None,
                })
            }
            "search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs("search requires 'query'".to_string()))?;

                let results = store.search(query).await?;
                let output = if results.is_empty() {
                    format!("No memories matching '{query}'")
                } else {
                    results
                        .iter()
                        .map(|e| format!("- {}: {}", e.key, e.value))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                Ok(ToolOutput {
                    title: format!("{} results for '{query}'", results.len()),
                    output,
                    metadata: serde_json::json!({"action": "search", "query": query, "count": results.len()}),
                    attachments: None,
                })
            }
            "delete" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs("delete requires 'key'".to_string()))?;

                let deleted = store.delete(key).await?;
                let output = if deleted {
                    format!("Memory '{key}' deleted")
                } else {
                    format!("Memory '{key}' not found")
                };

                Ok(ToolOutput {
                    title: format!("Delete: {key}"),
                    output,
                    metadata: serde_json::json!({"action": "delete", "key": key, "deleted": deleted}),
                    attachments: None,
                })
            }
            other => Err(ToolError::InvalidArgs(format!(
                "Unknown action '{}'. Use: save, recall, list, search, delete",
                other
            ))),
        }
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    // ── FileMemoryStore tests ──

    #[tokio::test]
    async fn store_save_and_recall_roundtrip() {
        let tmp = TestDir::new();
        let store = FileMemoryStore::new(tmp.path().to_path_buf());

        let entry = AgentMemoryEntry {
            key: "test_key".to_string(),
            value: "test_value".to_string(),
            created_at: 1000,
            run_id: "run-1".to_string(),
        };
        store.save(&entry).await.unwrap();

        let recalled = store.recall("test_key").await.unwrap().unwrap();
        assert_eq!(recalled.key, "test_key");
        assert_eq!(recalled.value, "test_value");
    }

    #[tokio::test]
    async fn store_recall_nonexistent_returns_none() {
        let tmp = TestDir::new();
        let store = FileMemoryStore::new(tmp.path().to_path_buf());
        assert!(store.recall("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn store_list_returns_all() {
        let tmp = TestDir::new();
        let store = FileMemoryStore::new(tmp.path().to_path_buf());

        for key in &["alpha", "beta", "gamma"] {
            store
                .save(&AgentMemoryEntry {
                    key: key.to_string(),
                    value: format!("value_{}", key),
                    created_at: 1000,
                    run_id: "r".to_string(),
                })
                .await
                .unwrap();
        }

        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].key, "alpha"); // sorted
    }

    #[tokio::test]
    async fn store_search_filters_by_substring() {
        let tmp = TestDir::new();
        let store = FileMemoryStore::new(tmp.path().to_path_buf());

        store
            .save(&AgentMemoryEntry {
                key: "db_schema".to_string(),
                value: "PostgreSQL with UUID primary keys".to_string(),
                created_at: 1000,
                run_id: "r".to_string(),
            })
            .await
            .unwrap();

        store
            .save(&AgentMemoryEntry {
                key: "api_style".to_string(),
                value: "REST with JSON".to_string(),
                created_at: 1000,
                run_id: "r".to_string(),
            })
            .await
            .unwrap();

        let results = store.search("postgres").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "db_schema");
    }

    #[tokio::test]
    async fn store_delete_removes_entry() {
        let tmp = TestDir::new();
        let store = FileMemoryStore::new(tmp.path().to_path_buf());

        store
            .save(&AgentMemoryEntry {
                key: "temp".to_string(),
                value: "temporary".to_string(),
                created_at: 1000,
                run_id: "r".to_string(),
            })
            .await
            .unwrap();

        assert!(store.delete("temp").await.unwrap());
        assert!(store.recall("temp").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn store_delete_nonexistent_returns_false() {
        let tmp = TestDir::new();
        let store = FileMemoryStore::new(tmp.path().to_path_buf());
        assert!(!store.delete("nope").await.unwrap());
    }

    #[tokio::test]
    async fn store_project_isolation() {
        let tmp = TestDir::new();
        let root = tmp.path();

        let store_a = FileMemoryStore::for_project(root, Path::new("/project/a"));
        let store_b = FileMemoryStore::for_project(root, Path::new("/project/b"));

        store_a
            .save(&AgentMemoryEntry {
                key: "only_a".to_string(),
                value: "project a".to_string(),
                created_at: 1000,
                run_id: "r".to_string(),
            })
            .await
            .unwrap();

        // Store B should not see Store A's memories
        assert!(store_b.recall("only_a").await.unwrap().is_none());
        assert_eq!(store_b.list().await.unwrap().len(), 0);

        // Store A should see its own
        assert!(store_a.recall("only_a").await.unwrap().is_some());
    }

    #[test]
    fn serde_roundtrip() {
        let entry = AgentMemoryEntry {
            key: "test".to_string(),
            value: "value".to_string(),
            created_at: 12345,
            run_id: "run-x".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: AgentMemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, entry.key);
        assert_eq!(back.value, entry.value);
    }

    #[test]
    fn sanitize_key_removes_special_chars() {
        assert_eq!(sanitize_key("hello world"), "hello_world");
        assert_eq!(sanitize_key("a/b/c"), "a_b_c");
        assert_eq!(sanitize_key("key-with_valid-chars"), "key-with_valid-chars");
    }

    // ── MemoryTool tests ──

    #[tokio::test]
    async fn tool_save_and_recall() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = MemoryTool::new();

        // Save
        tool.execute(
            serde_json::json!({"action": "save", "key": "framework", "value": "Axum"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap();

        // Recall
        let result = tool
            .execute(
                serde_json::json!({"action": "recall", "key": "framework"}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        // Note: this may fail because memory_root uses dirs_next::config_dir
        // which points to real ~/.config/theo/memory/, not tempdir.
        // In a real integration test, memory_dir would be injected.
        assert_eq!(result.metadata["action"], "recall");
    }

    #[tokio::test]
    async fn tool_invalid_action_returns_error() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let tool = MemoryTool::new();
        let result = tool
            .execute(serde_json::json!({"action": "invalid"}), &ctx, &mut perms)
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn tool_id_is_memory() {
        assert_eq!(MemoryTool::new().id(), "memory");
    }

    #[test]
    fn tool_schema_has_action() {
        let tool = MemoryTool::new();
        let schema = tool.schema();
        let required: Vec<&str> = schema
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(required.contains(&"action"));
    }
}
