use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use sysinfo::{System, Pid};
use std::process::Command;

/// Tool to list running processes
pub struct ProcessListTool;

#[async_trait]
impl Tool for ProcessListTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "list_processes",
            "List running processes with their PID, name, CPU and memory usage",
        )
        .with_param(ToolParameter::string(
            "filter",
            "Filter processes by name (optional, case-insensitive substring match)",
            false,
        ))
        .with_param(ToolParameter::number(
            "limit",
            "Maximum number of processes to return (default: 20)",
            false,
        ))
        .with_param(
            ToolParameter::string("sort_by", "Sort by: 'cpu', 'memory', 'name', 'pid'", false)
                .with_enum(vec![
                    "cpu".to_string(),
                    "memory".to_string(),
                    "name".to_string(),
                    "pid".to_string(),
                ]),
        )
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let filter = params.get("filter").and_then(|v| v.as_str());
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;
        let sort_by = params
            .get("sort_by")
            .and_then(|v| v.as_str())
            .unwrap_or("memory");

        let mut sys = System::new_all();
        sys.refresh_all();

        let mut processes: Vec<(Pid, String, f32, u64)> = sys
            .processes()
            .iter()
            .filter(|(_, process)| {
                if let Some(f) = filter {
                    process.name().to_string_lossy().to_lowercase().contains(&f.to_lowercase())
                } else {
                    true
                }
            })
            .map(|(pid, process)| {
                (
                    *pid,
                    process.name().to_string_lossy().to_string(),
                    process.cpu_usage(),
                    process.memory(),
                )
            })
            .collect();

        // Sort
        match sort_by {
            "cpu" => processes.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)),
            "memory" => processes.sort_by(|a, b| b.3.cmp(&a.3)),
            "name" => processes.sort_by(|a, b| a.1.cmp(&b.1)),
            "pid" => processes.sort_by(|a, b| a.0.cmp(&b.0)),
            _ => processes.sort_by(|a, b| b.3.cmp(&a.3)),
        }

        let processes: Vec<_> = processes.into_iter().take(limit).collect();

        let mut output = String::from("PID\tNAME\t\t\tCPU%\tMEM (MB)\n");
        output.push_str("-".repeat(60).as_str());
        output.push('\n');

        let mut process_list = Vec::new();

        for (pid, name, cpu, mem) in &processes {
            let mem_mb = *mem as f64 / 1_048_576.0;
            let display_name = if name.len() > 20 {
                format!("{}...", &name[..17])
            } else {
                format!("{:20}", name)
            };

            output.push_str(&format!(
                "{}\t{}\t{:.1}%\t{:.1}\n",
                pid, display_name, cpu, mem_mb
            ));

            process_list.push(serde_json::json!({
                "pid": pid.as_u32(),
                "name": name,
                "cpu_percent": format!("{:.1}", cpu),
                "memory_mb": format!("{:.1}", mem_mb)
            }));
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "total_shown": processes.len(),
                "processes": process_list
            }),
        ))
    }
}

/// Tool to launch applications
pub struct LaunchAppTool;

#[async_trait]
impl Tool for LaunchAppTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "launch_app",
            "Launch an application or executable by name or path",
        )
        .with_param(ToolParameter::string(
            "app",
            "Application name or full path to executable",
            true,
        ))
        .with_param(ToolParameter::array(
            "args",
            "Command line arguments (optional)",
            false,
        ))
        .with_param(ToolParameter::boolean(
            "detached",
            "Run in background without waiting (default: true)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let app = params
            .get("app")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Application name is required"))?;

        let args: Vec<String> = params
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let detached = params
            .get("detached")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Try to find the executable
        let app_path = if std::path::Path::new(app).exists() {
            app.to_string()
        } else {
            // Try to find in PATH
            which::which(app)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| app.to_string())
        };

        let mut cmd = Command::new(&app_path);
        cmd.args(&args);

        if detached {
            // Spawn and don't wait
            match cmd.spawn() {
                Ok(child) => Ok(ToolResult::success_with_data(
                    format!("Launched '{}' with PID {}", app, child.id()),
                    serde_json::json!({
                        "app": app,
                        "pid": child.id(),
                        "args": args,
                        "path": app_path
                    }),
                )),
                Err(e) => Ok(ToolResult::error(format!("Failed to launch '{}': {}", app, e))),
            }
        } else {
            // Wait for completion
            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                    Ok(ToolResult::success_with_data(
                        format!("Process completed\nOutput: {}\nErrors: {}", stdout, stderr),
                        serde_json::json!({
                            "app": app,
                            "exit_code": output.status.code(),
                            "stdout": stdout,
                            "stderr": stderr,
                            "success": output.status.success()
                        }),
                    ))
                }
                Err(e) => Ok(ToolResult::error(format!("Failed to run '{}': {}", app, e))),
            }
        }
    }
}

/// Tool to get information about currently focused window (Linux/X11 only for now)
pub struct ActiveWindowTool;

#[async_trait]
impl Tool for ActiveWindowTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "get_active_window",
            "Get information about the currently focused/active window",
        )
    }

    async fn execute(&self, _params: Value) -> Result<ToolResult> {
        #[cfg(target_os = "linux")]
        {
            // Try using xdotool on Linux
            let output = Command::new("xdotool")
                .args(["getactivewindow", "getwindowname"])
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let window_name = String::from_utf8_lossy(&out.stdout).trim().to_string();

                    // Also try to get window PID
                    let pid_output = Command::new("xdotool")
                        .args(["getactivewindow", "getwindowpid"])
                        .output();

                    let pid = pid_output
                        .ok()
                        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok());

                    Ok(ToolResult::success_with_data(
                        format!("Active window: {}", window_name),
                        serde_json::json!({
                            "window_name": window_name,
                            "pid": pid
                        }),
                    ))
                }
                _ => Ok(ToolResult::error(
                    "Could not get active window. Make sure xdotool is installed.",
                )),
            }
        }

        #[cfg(target_os = "windows")]
        {
            Ok(ToolResult::error(
                "Active window detection on Windows requires additional libraries. Consider using list_processes instead.",
            ))
        }

        #[cfg(target_os = "macos")]
        {
            // Try using osascript on macOS
            let output = Command::new("osascript")
                .args([
                    "-e",
                    "tell application \"System Events\" to get name of first process whose frontmost is true",
                ])
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let app_name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    Ok(ToolResult::success_with_data(
                        format!("Active application: {}", app_name),
                        serde_json::json!({
                            "application": app_name
                        }),
                    ))
                }
                _ => Ok(ToolResult::error("Could not get active application")),
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Ok(ToolResult::error("Active window detection not supported on this platform"))
        }
    }
}

/// Tool to kill a process
pub struct KillProcessTool;

#[async_trait]
impl Tool for KillProcessTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("kill_process", "Terminate a process by PID or name")
            .with_param(ToolParameter::number("pid", "Process ID to kill", false))
            .with_param(ToolParameter::string(
                "name",
                "Process name to kill (kills first matching)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let pid = params.get("pid").and_then(|v| v.as_u64());
        let name = params.get("name").and_then(|v| v.as_str());

        if pid.is_none() && name.is_none() {
            return Ok(ToolResult::error("Either pid or name is required"));
        }

        let mut sys = System::new_all();
        sys.refresh_all();

        let target_pid = if let Some(p) = pid {
            Some(Pid::from(p as usize))
        } else if let Some(n) = name {
            sys.processes()
                .iter()
                .find(|(_, process)| process.name().to_string_lossy().to_lowercase() == n.to_lowercase())
                .map(|(pid, _)| *pid)
        } else {
            None
        };

        match target_pid {
            Some(p) => {
                if let Some(process) = sys.process(p) {
                    let proc_name = process.name().to_string_lossy().to_string();
                    if process.kill() {
                        Ok(ToolResult::success(format!(
                            "Successfully killed process '{}' (PID: {})",
                            proc_name,
                            p.as_u32()
                        )))
                    } else {
                        Ok(ToolResult::error(format!(
                            "Failed to kill process '{}' (PID: {})",
                            proc_name,
                            p.as_u32()
                        )))
                    }
                } else {
                    Ok(ToolResult::error(format!("Process with PID {} not found", p.as_u32())))
                }
            }
            None => Ok(ToolResult::error("Process not found")),
        }
    }
}

pub fn register_process_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(ProcessListTool);
    registry.add_tool(LaunchAppTool);
    registry.add_tool(ActiveWindowTool);
    registry.add_tool(KillProcessTool);
}
