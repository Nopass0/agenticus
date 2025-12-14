use crate::config::Config;
use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// A memory entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: u32,
}

/// Memory storage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryStore {
    pub entries: HashMap<String, MemoryEntry>,
}

impl MemoryStore {
    pub fn load(path: &PathBuf) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let store: MemoryStore = serde_json::from_str(&content)?;
            Ok(store)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn set(&mut self, key: String, value: String, category: Option<String>) {
        let now = Utc::now();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.value = value;
            entry.updated_at = now;
            entry.category = category.or(entry.category.clone());
        } else {
            self.entries.insert(
                key.clone(),
                MemoryEntry {
                    key,
                    value,
                    category,
                    created_at: now,
                    updated_at: now,
                    access_count: 0,
                },
            );
        }
    }

    pub fn get(&mut self, key: &str) -> Option<MemoryEntry> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.access_count += 1;
            Some(entry.clone())
        } else {
            None
        }
    }

    pub fn search(&self, query: &str) -> Vec<&MemoryEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .values()
            .filter(|entry| {
                entry.key.to_lowercase().contains(&query_lower)
                    || entry.value.to_lowercase().contains(&query_lower)
                    || entry
                        .category
                        .as_ref()
                        .map(|c| c.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    pub fn list_by_category(&self, category: &str) -> Vec<&MemoryEntry> {
        let cat_lower = category.to_lowercase();
        self.entries
            .values()
            .filter(|entry| {
                entry
                    .category
                    .as_ref()
                    .map(|c| c.to_lowercase() == cat_lower)
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn delete(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    pub fn list_all(&self) -> Vec<&MemoryEntry> {
        self.entries.values().collect()
    }
}

/// Shared memory instance
pub type SharedMemory = Arc<RwLock<MemoryStore>>;

/// Get the memory file path
fn memory_path() -> PathBuf {
    Config::data_dir().join("memory.json")
}

/// Tool to save information to long-term memory
pub struct MemorySaveTool {
    memory: SharedMemory,
}

impl MemorySaveTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemorySaveTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "memory_save",
            "Save information to long-term memory for later retrieval. Use this to remember important facts, user preferences, or any information that should persist.",
        )
        .with_param(ToolParameter::string(
            "key",
            "Unique identifier for this memory (e.g., 'user_name', 'project_path')",
            true,
        ))
        .with_param(ToolParameter::string(
            "value",
            "The information to remember",
            true,
        ))
        .with_param(ToolParameter::string(
            "category",
            "Optional category to organize memories (e.g., 'preferences', 'facts', 'tasks')",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Key is required"))?
            .to_string();

        let value = params
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Value is required"))?
            .to_string();

        let category = params
            .get("category")
            .and_then(|v| v.as_str())
            .map(String::from);

        {
            let mut store = self.memory.write().unwrap();
            store.set(key.clone(), value.clone(), category.clone());
            store.save(&memory_path())?;
        }

        Ok(ToolResult::success_with_data(
            format!("Saved to memory: '{}' = '{}'", key, value),
            serde_json::json!({
                "key": key,
                "value": value,
                "category": category
            }),
        ))
    }
}

/// Tool to retrieve information from memory
pub struct MemoryGetTool {
    memory: SharedMemory,
}

impl MemoryGetTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryGetTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "memory_get",
            "Retrieve a specific piece of information from long-term memory by its key",
        )
        .with_param(ToolParameter::string(
            "key",
            "The key of the memory to retrieve",
            true,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Key is required"))?;

        let mut store = self.memory.write().unwrap();
        if let Some(entry) = store.get(key) {
            let result = serde_json::json!({
                "key": entry.key,
                "value": entry.value,
                "category": entry.category,
                "created_at": entry.created_at.to_rfc3339(),
                "updated_at": entry.updated_at.to_rfc3339(),
                "access_count": entry.access_count
            });
            let value = entry.value.clone();
            store.save(&memory_path())?;
            Ok(ToolResult::success_with_data(value, result))
        } else {
            Ok(ToolResult::error(format!("Memory '{}' not found", key)))
        }
    }
}

/// Tool to search through memory
pub struct MemorySearchTool {
    memory: SharedMemory,
}

impl MemorySearchTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "memory_search",
            "Search through long-term memory for matching entries. Searches in keys, values, and categories.",
        )
        .with_param(ToolParameter::string(
            "query",
            "Search query (case-insensitive)",
            true,
        ))
        .with_param(ToolParameter::string(
            "category",
            "Optional: only search within a specific category",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Query is required"))?;

        let category = params.get("category").and_then(|v| v.as_str());

        let store = self.memory.read().unwrap();

        let results: Vec<_> = if let Some(cat) = category {
            store
                .list_by_category(cat)
                .into_iter()
                .filter(|e| {
                    e.key.to_lowercase().contains(&query.to_lowercase())
                        || e.value.to_lowercase().contains(&query.to_lowercase())
                })
                .collect()
        } else {
            store.search(query)
        };

        if results.is_empty() {
            Ok(ToolResult::success("No matching memories found"))
        } else {
            let mut output = format!("Found {} matching memories:\n\n", results.len());
            let mut json_results = Vec::new();

            for entry in results {
                output.push_str(&format!("• {}: {}\n", entry.key, entry.value));
                json_results.push(serde_json::json!({
                    "key": entry.key,
                    "value": entry.value,
                    "category": entry.category
                }));
            }

            Ok(ToolResult::success_with_data(
                output,
                serde_json::json!({ "results": json_results }),
            ))
        }
    }
}

/// Tool to list all memories
pub struct MemoryListTool {
    memory: SharedMemory,
}

impl MemoryListTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryListTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "memory_list",
            "List all memories, optionally filtered by category",
        )
        .with_param(ToolParameter::string(
            "category",
            "Optional: only list memories in this category",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let category = params.get("category").and_then(|v| v.as_str());

        let store = self.memory.read().unwrap();

        let entries: Vec<_> = if let Some(cat) = category {
            store.list_by_category(cat)
        } else {
            store.list_all()
        };

        if entries.is_empty() {
            Ok(ToolResult::success("No memories stored"))
        } else {
            let mut output = format!("Stored memories ({}):\n\n", entries.len());
            let mut json_results = Vec::new();

            for entry in entries {
                let cat_str = entry
                    .category
                    .as_ref()
                    .map(|c| format!(" [{}]", c))
                    .unwrap_or_default();
                output.push_str(&format!("• {}{}: {}\n", entry.key, cat_str, entry.value));
                json_results.push(serde_json::json!({
                    "key": entry.key,
                    "value": entry.value,
                    "category": entry.category
                }));
            }

            Ok(ToolResult::success_with_data(
                output,
                serde_json::json!({ "memories": json_results }),
            ))
        }
    }
}

/// Tool to delete a memory
pub struct MemoryDeleteTool {
    memory: SharedMemory,
}

impl MemoryDeleteTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryDeleteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("memory_delete", "Delete a memory by its key")
            .with_param(ToolParameter::string(
                "key",
                "The key of the memory to delete",
                true,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Key is required"))?;

        let mut store = self.memory.write().unwrap();
        if store.delete(key) {
            store.save(&memory_path())?;
            Ok(ToolResult::success(format!("Deleted memory '{}'", key)))
        } else {
            Ok(ToolResult::error(format!("Memory '{}' not found", key)))
        }
    }
}

pub fn create_shared_memory() -> SharedMemory {
    let store = MemoryStore::load(&memory_path()).unwrap_or_default();
    Arc::new(RwLock::new(store))
}

pub fn register_memory_tools(registry: &mut crate::tools::ToolRegistry, memory: SharedMemory) {
    registry.add_tool(MemorySaveTool::new(memory.clone()));
    registry.add_tool(MemoryGetTool::new(memory.clone()));
    registry.add_tool(MemorySearchTool::new(memory.clone()));
    registry.add_tool(MemoryListTool::new(memory.clone()));
    registry.add_tool(MemoryDeleteTool::new(memory));
}
