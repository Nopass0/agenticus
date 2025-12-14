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

/// Tool to save a session summary to long-term memory
pub struct SaveSessionSummaryTool {
    memory: SharedMemory,
}

impl SaveSessionSummaryTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for SaveSessionSummaryTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "save_session_summary",
            "Save a summary of the current session/conversation to long-term memory. Use at the end of a session to remember key findings and context.",
        )
        .with_param(ToolParameter::string(
            "summary",
            "A concise summary of what was accomplished, key findings, or important context",
            true,
        ))
        .with_param(ToolParameter::string(
            "topic",
            "The main topic or task name for this session",
            true,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let summary = params
            .get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Summary is required"))?
            .to_string();

        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Topic is required"))?
            .to_string();

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let key = format!("session_{}", timestamp);

        let full_summary = format!("Topic: {}\n\n{}", topic, summary);

        {
            let mut store = self.memory.write().unwrap();
            store.set(key.clone(), full_summary.clone(), Some("sessions".to_string()));
            store.save(&memory_path())?;
        }

        Ok(ToolResult::success_with_data(
            format!("Session summary saved with key '{}'", key),
            serde_json::json!({
                "key": key,
                "topic": topic,
                "summary": summary
            }),
        ))
    }
}

/// Tool to get recent session summaries
pub struct GetRecentSessionsTool {
    memory: SharedMemory,
}

impl GetRecentSessionsTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for GetRecentSessionsTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "get_recent_sessions",
            "Get summaries of recent sessions to understand past context and work",
        )
        .with_param(ToolParameter::number(
            "limit",
            "Number of recent sessions to retrieve (default: 5)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        let store = self.memory.read().unwrap();
        let mut sessions: Vec<_> = store
            .list_by_category("sessions")
            .into_iter()
            .collect();

        // Sort by key (which contains timestamp) descending
        sessions.sort_by(|a, b| b.key.cmp(&a.key));
        sessions.truncate(limit);

        if sessions.is_empty() {
            Ok(ToolResult::success("No session summaries found"))
        } else {
            let mut output = format!("Recent {} session summaries:\n\n", sessions.len());
            let mut json_results = Vec::new();

            for entry in &sessions {
                output.push_str(&format!("=== {} ===\n{}\n\n", entry.key, entry.value));
                json_results.push(serde_json::json!({
                    "key": entry.key,
                    "summary": entry.value,
                    "created_at": entry.created_at.to_rfc3339()
                }));
            }

            Ok(ToolResult::success_with_data(
                output,
                serde_json::json!({ "sessions": json_results }),
            ))
        }
    }
}

/// Tool to save a custom tool to memory
pub struct SaveCustomToolTool {
    memory: SharedMemory,
}

impl SaveCustomToolTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for SaveCustomToolTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "save_custom_tool",
            "Save a custom tool/script to memory for reuse. The tool will be available in future sessions.",
        )
        .with_param(ToolParameter::string("name", "Unique name for the tool", true))
        .with_param(ToolParameter::string("description", "What the tool does", true))
        .with_param(ToolParameter::string("language", "Programming language: python, javascript, bash, rust, etc.", true))
        .with_param(ToolParameter::string("code", "The tool's source code", true))
        .with_param(ToolParameter::string("usage", "Example usage or parameters", false))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Name required"))?;
        let description = params.get("description").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Description required"))?;
        let language = params.get("language").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Language required"))?;
        let code = params.get("code").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Code required"))?;
        let usage = params.get("usage").and_then(|v| v.as_str()).unwrap_or("");

        let tool_data = serde_json::json!({
            "name": name,
            "description": description,
            "language": language,
            "code": code,
            "usage": usage,
            "created_at": Utc::now().to_rfc3339()
        });

        let key = format!("tool_{}", name);
        let value = serde_json::to_string_pretty(&tool_data)?;

        {
            let mut store = self.memory.write().unwrap();
            store.set(key.clone(), value, Some("custom_tools".to_string()));
            store.save(&memory_path())?;
        }

        Ok(ToolResult::success_with_data(
            format!("Custom tool '{}' saved. Use 'run_custom_tool' to execute it.", name),
            tool_data,
        ))
    }
}

/// Tool to list available custom tools
pub struct ListCustomToolsTool {
    memory: SharedMemory,
}

impl ListCustomToolsTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for ListCustomToolsTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "list_custom_tools",
            "List all custom tools saved to memory",
        )
    }

    async fn execute(&self, _params: Value) -> Result<ToolResult> {
        let store = self.memory.read().unwrap();
        let tools: Vec<_> = store.list_by_category("custom_tools");

        if tools.is_empty() {
            Ok(ToolResult::success("No custom tools found. Use 'save_custom_tool' to create one."))
        } else {
            let mut output = format!("Available custom tools ({}):\n\n", tools.len());
            let mut json_results = Vec::new();

            for entry in &tools {
                if let Ok(tool_data) = serde_json::from_str::<serde_json::Value>(&entry.value) {
                    let name = tool_data["name"].as_str().unwrap_or(&entry.key);
                    let desc = tool_data["description"].as_str().unwrap_or("");
                    let lang = tool_data["language"].as_str().unwrap_or("");
                    output.push_str(&format!("• {} [{}]: {}\n", name, lang, desc));
                    json_results.push(tool_data);
                }
            }

            Ok(ToolResult::success_with_data(
                output,
                serde_json::json!({ "tools": json_results }),
            ))
        }
    }
}

/// Tool to run a custom tool
pub struct RunCustomToolTool {
    memory: SharedMemory,
}

impl RunCustomToolTool {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for RunCustomToolTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "run_custom_tool",
            "Run a custom tool that was previously saved to memory",
        )
        .with_param(ToolParameter::string("name", "Name of the custom tool to run", true))
        .with_param(ToolParameter::string("args", "Arguments to pass to the tool (as JSON or space-separated)", false))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Name required"))?;
        let args = params.get("args").and_then(|v| v.as_str()).unwrap_or("");

        let key = format!("tool_{}", name);

        let tool_data = {
            let store = self.memory.read().unwrap();
            store.entries.get(&key).map(|e| e.value.clone())
        };

        let tool_json: serde_json::Value = match tool_data {
            Some(data) => serde_json::from_str(&data)?,
            None => return Ok(ToolResult::error(format!("Custom tool '{}' not found", name))),
        };

        let language = tool_json["language"].as_str().unwrap_or("python");
        let code = tool_json["code"].as_str().ok_or_else(|| anyhow::anyhow!("Tool has no code"))?;

        // Determine file extension and runner
        let (extension, runner, runner_args): (&str, &str, Vec<&str>) = match language.to_lowercase().as_str() {
            "python" | "py" => ("py", "python3", vec![]),
            "javascript" | "js" | "node" => ("js", "node", vec![]),
            "bash" | "sh" => ("sh", "bash", vec![]),
            "ruby" | "rb" => ("rb", "ruby", vec![]),
            _ => ("py", "python3", vec![]),
        };

        let filename = format!("/tmp/custom_tool_{}.{}", std::process::id(), extension);

        // Write the script file
        std::fs::write(&filename, code)?;

        // Make executable for shell scripts
        #[cfg(unix)]
        if extension == "sh" {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&filename, std::fs::Permissions::from_mode(0o755));
        }

        // Run the script
        let mut cmd = std::process::Command::new(runner);
        for arg in &runner_args {
            cmd.arg(arg);
        }
        cmd.arg(&filename);

        // Add arguments
        if !args.is_empty() {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let output = cmd.output();

        // Clean up
        let _ = std::fs::remove_file(&filename);

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();

                if o.status.success() {
                    Ok(ToolResult::success_with_data(
                        if stdout.is_empty() { "Tool executed successfully".to_string() } else { stdout.clone() },
                        serde_json::json!({
                            "tool": name,
                            "stdout": stdout,
                            "stderr": stderr,
                            "success": true
                        }),
                    ))
                } else {
                    let error_output = if stderr.is_empty() { stdout } else { stderr };
                    Ok(ToolResult::error(format!("Tool '{}' failed:\n{}", name, error_output)))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to run tool: {}", e))),
        }
    }
}

pub fn register_memory_tools(registry: &mut crate::tools::ToolRegistry, memory: SharedMemory) {
    registry.add_tool(MemorySaveTool::new(memory.clone()));
    registry.add_tool(MemoryGetTool::new(memory.clone()));
    registry.add_tool(MemorySearchTool::new(memory.clone()));
    registry.add_tool(MemoryListTool::new(memory.clone()));
    registry.add_tool(MemoryDeleteTool::new(memory.clone()));
    registry.add_tool(SaveSessionSummaryTool::new(memory.clone()));
    registry.add_tool(GetRecentSessionsTool::new(memory.clone()));
    registry.add_tool(SaveCustomToolTool::new(memory.clone()));
    registry.add_tool(ListCustomToolsTool::new(memory.clone()));
    registry.add_tool(RunCustomToolTool::new(memory));
}
