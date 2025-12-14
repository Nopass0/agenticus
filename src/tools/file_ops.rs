use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Helper to safely truncate UTF-8 strings
fn safe_truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Tool to read file contents
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("read_file", "Read the contents of a file")
            .with_param(ToolParameter::string("path", "Path to the file to read", true))
            .with_param(ToolParameter::number(
                "max_length",
                "Maximum number of characters to read (default: 10000)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Path is required"))?;

        let max_length = params
            .get("max_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000) as usize;

        let path = Path::new(path);

        if !path.exists() {
            return Ok(ToolResult::error(format!("File not found: {}", path.display())));
        }

        match fs::read_to_string(path) {
            Ok(content) => {
                let truncated = content.len() > max_length;
                let display_content = if truncated {
                    format!("{}... [truncated, showing {}/{} chars]",
                        safe_truncate(&content, max_length),
                        max_length,
                        content.len()
                    )
                } else {
                    content.clone()
                };

                Ok(ToolResult::success_with_data(
                    display_content,
                    serde_json::json!({
                        "path": path.display().to_string(),
                        "size": content.len(),
                        "truncated": truncated
                    }),
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        }
    }
}

/// Tool to write content to a file
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("write_file", "Write content to a file (creates file if it doesn't exist)")
            .with_param(ToolParameter::string("path", "Path to the file to write", true))
            .with_param(ToolParameter::string("content", "Content to write to the file", true))
            .with_param(ToolParameter::boolean(
                "append",
                "Append to file instead of overwriting (default: false)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Path is required"))?;

        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Content is required"))?;

        let append = params
            .get("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = Path::new(path);

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Ok(ToolResult::error(format!(
                        "Failed to create directory: {}",
                        e
                    )));
                }
            }
        }

        let result = if append {
            use std::fs::OpenOptions;
            use std::io::Write;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(content.as_bytes()))
        } else {
            fs::write(path, content)
        };

        match result {
            Ok(_) => Ok(ToolResult::success_with_data(
                format!(
                    "Successfully {} {} bytes to {}",
                    if append { "appended" } else { "wrote" },
                    content.len(),
                    path.display()
                ),
                serde_json::json!({
                    "path": path.display().to_string(),
                    "bytes_written": content.len(),
                    "append": append
                }),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {}", e))),
        }
    }
}

/// Tool to list directory contents
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("list_dir", "List contents of a directory")
            .with_param(ToolParameter::string(
                "path",
                "Path to the directory (default: current directory)",
                false,
            ))
            .with_param(ToolParameter::boolean(
                "show_hidden",
                "Show hidden files (default: false)",
                false,
            ))
            .with_param(ToolParameter::boolean(
                "details",
                "Show file details like size and modification time (default: false)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let show_hidden = params
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let details = params
            .get("details")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = Path::new(path);

        if !path.exists() {
            return Ok(ToolResult::error(format!(
                "Directory not found: {}",
                path.display()
            )));
        }

        if !path.is_dir() {
            return Ok(ToolResult::error(format!(
                "Not a directory: {}",
                path.display()
            )));
        }

        let mut entries = Vec::new();
        let mut output = String::new();

        match fs::read_dir(path) {
            Ok(dir) => {
                for entry in dir.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();

                    // Skip hidden files if not requested
                    if !show_hidden && name.starts_with('.') {
                        continue;
                    }

                    let file_type = if entry.path().is_dir() {
                        "dir"
                    } else {
                        "file"
                    };

                    if details {
                        let metadata = entry.metadata().ok();
                        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                        let modified = metadata
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .map(|t| {
                                chrono::DateTime::<chrono::Local>::from(t)
                                    .format("%Y-%m-%d %H:%M")
                                    .to_string()
                            })
                            .unwrap_or_else(|| "unknown".to_string());

                        entries.push(serde_json::json!({
                            "name": name,
                            "type": file_type,
                            "size": size,
                            "modified": modified
                        }));

                        let size_str = if entry.path().is_dir() {
                            "<DIR>".to_string()
                        } else {
                            format_size(size)
                        };

                        output.push_str(&format!(
                            "{:>10}  {}  {}\n",
                            size_str, modified, name
                        ));
                    } else {
                        entries.push(serde_json::json!({
                            "name": name,
                            "type": file_type
                        }));

                        let prefix = if entry.path().is_dir() { "[D] " } else { "    " };
                        output.push_str(&format!("{}{}\n", prefix, name));
                    }
                }

                Ok(ToolResult::success_with_data(
                    output,
                    serde_json::json!({
                        "path": path.display().to_string(),
                        "count": entries.len(),
                        "entries": entries
                    }),
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to read directory: {}", e))),
        }
    }
}

/// Tool to create a directory
pub struct CreateDirTool;

#[async_trait]
impl Tool for CreateDirTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("create_dir", "Create a directory (creates parent directories if needed)")
            .with_param(ToolParameter::string("path", "Path of the directory to create", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Path is required"))?;

        let path = Path::new(path);

        if path.exists() {
            return Ok(ToolResult::success(format!(
                "Directory already exists: {}",
                path.display()
            )));
        }

        match fs::create_dir_all(path) {
            Ok(_) => Ok(ToolResult::success_with_data(
                format!("Created directory: {}", path.display()),
                serde_json::json!({
                    "path": path.display().to_string(),
                    "created": true
                }),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to create directory: {}", e))),
        }
    }
}

/// Tool to delete a file or directory
pub struct DeletePathTool;

#[async_trait]
impl Tool for DeletePathTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("delete_path", "Delete a file or directory")
            .with_param(ToolParameter::string("path", "Path to delete", true))
            .with_param(ToolParameter::boolean(
                "recursive",
                "Recursively delete directories (default: false)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Path is required"))?;

        let recursive = params
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = Path::new(path);

        if !path.exists() {
            return Ok(ToolResult::error(format!("Path not found: {}", path.display())));
        }

        let result = if path.is_dir() {
            if recursive {
                fs::remove_dir_all(path)
            } else {
                fs::remove_dir(path)
            }
        } else {
            fs::remove_file(path)
        };

        match result {
            Ok(_) => Ok(ToolResult::success_with_data(
                format!("Deleted: {}", path.display()),
                serde_json::json!({
                    "path": path.display().to_string(),
                    "deleted": true
                }),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to delete: {}", e))),
        }
    }
}

/// Tool to copy a file
pub struct CopyFileTool;

#[async_trait]
impl Tool for CopyFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("copy_file", "Copy a file to a new location")
            .with_param(ToolParameter::string("source", "Source file path", true))
            .with_param(ToolParameter::string("destination", "Destination file path", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Source is required"))?;

        let destination = params
            .get("destination")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Destination is required"))?;

        let source = Path::new(source);
        let destination = Path::new(destination);

        if !source.exists() {
            return Ok(ToolResult::error(format!(
                "Source file not found: {}",
                source.display()
            )));
        }

        // Create parent directories for destination
        if let Some(parent) = destination.parent() {
            if !parent.exists() {
                let _ = fs::create_dir_all(parent);
            }
        }

        match fs::copy(source, destination) {
            Ok(bytes) => Ok(ToolResult::success_with_data(
                format!(
                    "Copied {} to {} ({} bytes)",
                    source.display(),
                    destination.display(),
                    bytes
                ),
                serde_json::json!({
                    "source": source.display().to_string(),
                    "destination": destination.display().to_string(),
                    "bytes_copied": bytes
                }),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to copy: {}", e))),
        }
    }
}

/// Tool to move/rename a file
pub struct MoveFileTool;

#[async_trait]
impl Tool for MoveFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("move_file", "Move or rename a file")
            .with_param(ToolParameter::string("source", "Source file path", true))
            .with_param(ToolParameter::string("destination", "Destination file path", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Source is required"))?;

        let destination = params
            .get("destination")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Destination is required"))?;

        let source = Path::new(source);
        let destination = Path::new(destination);

        if !source.exists() {
            return Ok(ToolResult::error(format!(
                "Source not found: {}",
                source.display()
            )));
        }

        // Create parent directories for destination
        if let Some(parent) = destination.parent() {
            if !parent.exists() {
                let _ = fs::create_dir_all(parent);
            }
        }

        match fs::rename(source, destination) {
            Ok(_) => Ok(ToolResult::success_with_data(
                format!("Moved {} to {}", source.display(), destination.display()),
                serde_json::json!({
                    "source": source.display().to_string(),
                    "destination": destination.display().to_string()
                }),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to move: {}", e))),
        }
    }
}

/// Tool to get file info
pub struct FileInfoTool;

#[async_trait]
impl Tool for FileInfoTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("file_info", "Get detailed information about a file or directory")
            .with_param(ToolParameter::string("path", "Path to the file or directory", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Path is required"))?;

        let path = Path::new(path);

        if !path.exists() {
            return Ok(ToolResult::error(format!("Path not found: {}", path.display())));
        }

        match fs::metadata(path) {
            Ok(metadata) => {
                let file_type = if metadata.is_dir() {
                    "directory"
                } else if metadata.is_file() {
                    "file"
                } else if metadata.is_symlink() {
                    "symlink"
                } else {
                    "unknown"
                };

                let modified = metadata
                    .modified()
                    .ok()
                    .map(|t| {
                        chrono::DateTime::<chrono::Local>::from(t)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                let created = metadata
                    .created()
                    .ok()
                    .map(|t| {
                        chrono::DateTime::<chrono::Local>::from(t)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                let size = metadata.len();

                let output = format!(
                    "Path: {}\nType: {}\nSize: {} ({})\nModified: {}\nCreated: {}",
                    path.display(),
                    file_type,
                    format_size(size),
                    size,
                    modified,
                    created
                );

                Ok(ToolResult::success_with_data(
                    output,
                    serde_json::json!({
                        "path": path.display().to_string(),
                        "type": file_type,
                        "size": size,
                        "size_formatted": format_size(size),
                        "modified": modified,
                        "created": created,
                        "readonly": metadata.permissions().readonly()
                    }),
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to get file info: {}", e))),
        }
    }
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

pub fn register_file_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(ReadFileTool);
    registry.add_tool(WriteFileTool);
    registry.add_tool(ListDirTool);
    registry.add_tool(CreateDirTool);
    registry.add_tool(DeletePathTool);
    registry.add_tool(CopyFileTool);
    registry.add_tool(MoveFileTool);
    registry.add_tool(FileInfoTool);
}
