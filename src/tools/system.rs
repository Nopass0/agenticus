use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Local, Utc};
use serde_json::Value;
use sysinfo::System;
use std::env;

/// Tool to get current date and time
pub struct DateTimeTool;

#[async_trait]
impl Tool for DateTimeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "get_datetime",
            "Get current date and time in various formats and timezones",
        )
        .with_param(
            ToolParameter::string("timezone", "Timezone: 'local' or 'utc'", false)
                .with_enum(vec!["local".to_string(), "utc".to_string()]),
        )
        .with_param(ToolParameter::string(
            "format",
            "Output format: 'full', 'date', 'time', 'iso'",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let timezone = params
            .get("timezone")
            .and_then(|v| v.as_str())
            .unwrap_or("local");

        let format = params
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("full");

        let (formatted, dt_json) = match timezone {
            "utc" => {
                let now: DateTime<Utc> = Utc::now();
                let formatted = match format {
                    "date" => now.format("%Y-%m-%d").to_string(),
                    "time" => now.format("%H:%M:%S").to_string(),
                    "iso" => now.to_rfc3339(),
                    _ => now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                };
                let json = serde_json::json!({
                    "year": now.format("%Y").to_string(),
                    "month": now.format("%m").to_string(),
                    "day": now.format("%d").to_string(),
                    "hour": now.format("%H").to_string(),
                    "minute": now.format("%M").to_string(),
                    "second": now.format("%S").to_string(),
                    "weekday": now.format("%A").to_string(),
                    "timezone": "UTC",
                    "timestamp": now.timestamp()
                });
                (formatted, json)
            }
            _ => {
                let now: DateTime<Local> = Local::now();
                let formatted = match format {
                    "date" => now.format("%Y-%m-%d").to_string(),
                    "time" => now.format("%H:%M:%S").to_string(),
                    "iso" => now.to_rfc3339(),
                    _ => now.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
                };
                let json = serde_json::json!({
                    "year": now.format("%Y").to_string(),
                    "month": now.format("%m").to_string(),
                    "day": now.format("%d").to_string(),
                    "hour": now.format("%H").to_string(),
                    "minute": now.format("%M").to_string(),
                    "second": now.format("%S").to_string(),
                    "weekday": now.format("%A").to_string(),
                    "timezone": now.format("%Z").to_string(),
                    "timestamp": now.timestamp()
                });
                (formatted, json)
            }
        };

        Ok(ToolResult::success_with_data(formatted, dt_json))
    }
}

/// Tool to get system/OS information
pub struct SystemInfoTool;

#[async_trait]
impl Tool for SystemInfoTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "get_system_info",
            "Get detailed information about the operating system, hardware, and system resources",
        )
        .with_param(
            ToolParameter::string(
                "category",
                "What info to get: 'all', 'os', 'cpu', 'memory', 'disk', 'network', 'user'",
                false,
            )
            .with_enum(vec![
                "all".to_string(),
                "os".to_string(),
                "cpu".to_string(),
                "memory".to_string(),
                "disk".to_string(),
                "user".to_string(),
            ]),
        )
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let category = params
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        let mut sys = System::new_all();
        sys.refresh_all();

        let mut info = serde_json::Map::new();
        let mut output = String::new();

        // OS Info
        if category == "all" || category == "os" {
            let os_info = serde_json::json!({
                "name": System::name().unwrap_or_default(),
                "kernel_version": System::kernel_version().unwrap_or_default(),
                "os_version": System::os_version().unwrap_or_default(),
                "host_name": System::host_name().unwrap_or_default(),
                "arch": std::env::consts::ARCH,
            });
            info.insert("os".to_string(), os_info);
            output.push_str(&format!(
                "OS: {} {}\nKernel: {}\nHostname: {}\nArch: {}\n",
                System::name().unwrap_or_default(),
                System::os_version().unwrap_or_default(),
                System::kernel_version().unwrap_or_default(),
                System::host_name().unwrap_or_default(),
                std::env::consts::ARCH
            ));
        }

        // CPU Info
        if category == "all" || category == "cpu" {
            let cpus = sys.cpus();
            let cpu_count = cpus.len();
            let cpu_usage: f32 = cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpu_count as f32;

            let cpu_info = serde_json::json!({
                "count": cpu_count,
                "brand": cpus.first().map(|c| c.brand()).unwrap_or("Unknown"),
                "usage_percent": format!("{:.1}", cpu_usage),
                "frequency_mhz": cpus.first().map(|c| c.frequency()).unwrap_or(0),
            });
            info.insert("cpu".to_string(), cpu_info);
            output.push_str(&format!(
                "CPU: {} cores, {:.1}% usage\n",
                cpu_count, cpu_usage
            ));
        }

        // Memory Info
        if category == "all" || category == "memory" {
            let total_mem = sys.total_memory();
            let used_mem = sys.used_memory();
            let free_mem = sys.available_memory();

            let mem_info = serde_json::json!({
                "total_bytes": total_mem,
                "total_gb": format!("{:.2}", total_mem as f64 / 1_073_741_824.0),
                "used_bytes": used_mem,
                "used_gb": format!("{:.2}", used_mem as f64 / 1_073_741_824.0),
                "free_bytes": free_mem,
                "free_gb": format!("{:.2}", free_mem as f64 / 1_073_741_824.0),
                "usage_percent": format!("{:.1}", (used_mem as f64 / total_mem as f64) * 100.0),
            });
            info.insert("memory".to_string(), mem_info);
            output.push_str(&format!(
                "Memory: {:.2} GB / {:.2} GB ({:.1}% used)\n",
                used_mem as f64 / 1_073_741_824.0,
                total_mem as f64 / 1_073_741_824.0,
                (used_mem as f64 / total_mem as f64) * 100.0
            ));
        }

        // Disk Info
        if category == "all" || category == "disk" {
            let disks = sysinfo::Disks::new_with_refreshed_list();
            let mut disk_list = Vec::new();

            for disk in disks.list() {
                let total = disk.total_space();
                let available = disk.available_space();
                let used = total - available;

                disk_list.push(serde_json::json!({
                    "name": disk.name().to_string_lossy(),
                    "mount_point": disk.mount_point().to_string_lossy(),
                    "file_system": String::from_utf8_lossy(disk.file_system().as_encoded_bytes()),
                    "total_gb": format!("{:.2}", total as f64 / 1_073_741_824.0),
                    "used_gb": format!("{:.2}", used as f64 / 1_073_741_824.0),
                    "available_gb": format!("{:.2}", available as f64 / 1_073_741_824.0),
                }));

                output.push_str(&format!(
                    "Disk {}: {:.2} GB / {:.2} GB available\n",
                    disk.mount_point().to_string_lossy(),
                    available as f64 / 1_073_741_824.0,
                    total as f64 / 1_073_741_824.0
                ));
            }
            info.insert("disks".to_string(), Value::Array(disk_list));
        }

        // User Info
        if category == "all" || category == "user" {
            let user_info = serde_json::json!({
                "username": env::var("USER").or_else(|_| env::var("USERNAME")).unwrap_or_default(),
                "home_dir": dirs::home_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                "current_dir": env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            });
            info.insert("user".to_string(), user_info);
            output.push_str(&format!(
                "User: {}\nHome: {}\n",
                env::var("USER").or_else(|_| env::var("USERNAME")).unwrap_or_default(),
                dirs::home_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
            ));
        }

        Ok(ToolResult::success_with_data(output, Value::Object(info)))
    }
}

/// Tool to get/set environment variables
pub struct EnvVarTool;

#[async_trait]
impl Tool for EnvVarTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("env_var", "Get or list environment variables")
            .with_param(ToolParameter::string(
                "name",
                "Name of the environment variable to get (optional, lists all if not provided)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let name = params.get("name").and_then(|v| v.as_str());

        match name {
            Some(var_name) => {
                match env::var(var_name) {
                    Ok(value) => Ok(ToolResult::success_with_data(
                        format!("{}={}", var_name, value),
                        serde_json::json!({ "name": var_name, "value": value }),
                    )),
                    Err(_) => Ok(ToolResult::error(format!(
                        "Environment variable '{}' not found",
                        var_name
                    ))),
                }
            }
            None => {
                let vars: Vec<Value> = env::vars()
                    .take(50) // Limit to 50 vars
                    .map(|(k, v)| serde_json::json!({ "name": k, "value": v }))
                    .collect();

                let output = vars
                    .iter()
                    .filter_map(|v| {
                        Some(format!(
                            "{}={}",
                            v.get("name")?.as_str()?,
                            v.get("value")?.as_str()?
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(ToolResult::success_with_data(
                    format!("Environment variables (first 50):\n{}", output),
                    Value::Array(vars),
                ))
            }
        }
    }
}

/// Tool to run shell commands
pub struct ShellCommandTool;

#[async_trait]
impl Tool for ShellCommandTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "run_command",
            "Execute a shell command and return the output. Use with caution.",
        )
        .with_param(ToolParameter::string("command", "The command to execute", true))
        .with_param(ToolParameter::string(
            "working_dir",
            "Working directory for the command (optional)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Command is required"))?;

        let working_dir = params.get("working_dir").and_then(|v| v.as_str());

        let shell = if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        };

        let shell_arg = if cfg!(target_os = "windows") {
            "/C"
        } else {
            "-c"
        };

        let mut cmd = tokio::process::Command::new(shell);
        cmd.arg(shell_arg).arg(command);

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        let output = cmd.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let result = serde_json::json!({
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "success": output.status.success()
        });

        if output.status.success() {
            let display = if stdout.is_empty() {
                "(no output)".to_string()
            } else {
                stdout.clone()
            };
            Ok(ToolResult::success_with_data(display, result))
        } else {
            Ok(ToolResult::success_with_data(
                format!("Command failed:\n{}\n{}", stdout, stderr),
                result,
            ))
        }
    }
}

pub fn register_system_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(DateTimeTool);
    registry.add_tool(SystemInfoTool);
    registry.add_tool(EnvVarTool);
    registry.add_tool(ShellCommandTool);
}
