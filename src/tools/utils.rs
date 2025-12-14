use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::process::Command;
use std::time::Duration;

/// Tool to copy text to clipboard
pub struct ClipboardCopyTool;

#[async_trait]
impl Tool for ClipboardCopyTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("clipboard_copy", "Copy text to the system clipboard")
            .with_param(ToolParameter::string("text", "Text to copy to clipboard", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Text is required"))?;

        #[cfg(target_os = "windows")]
        {
            // Use PowerShell to set clipboard
            let script = format!(
                "Set-Clipboard -Value '{}'",
                text.replace("'", "''")
            );
            let output = Command::new("powershell")
                .args(["-Command", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Copied {} characters to clipboard", text.len())))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "Failed to copy: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to execute: {}", e))),
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Try xclip first, then xsel
            use std::io::Write;
            use std::process::Stdio;

            let result = Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                });

            match result {
                Ok(status) if status.success() => {
                    Ok(ToolResult::success(format!("Copied {} characters to clipboard", text.len())))
                }
                _ => {
                    // Try xsel as fallback
                    let result = Command::new("xsel")
                        .args(["--clipboard", "--input"])
                        .stdin(Stdio::piped())
                        .spawn()
                        .and_then(|mut child| {
                            if let Some(stdin) = child.stdin.as_mut() {
                                stdin.write_all(text.as_bytes())?;
                            }
                            child.wait()
                        });

                    match result {
                        Ok(status) if status.success() => {
                            Ok(ToolResult::success(format!("Copied {} characters to clipboard", text.len())))
                        }
                        _ => Ok(ToolResult::error("Failed to copy. Install xclip or xsel.")),
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            use std::process::Stdio;

            let result = Command::new("pbcopy")
                .stdin(Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                });

            match result {
                Ok(status) if status.success() => {
                    Ok(ToolResult::success(format!("Copied {} characters to clipboard", text.len())))
                }
                _ => Ok(ToolResult::error("Failed to copy to clipboard")),
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult::error("Clipboard not supported on this platform"))
        }
    }
}

/// Tool to read text from clipboard
pub struct ClipboardPasteTool;

#[async_trait]
impl Tool for ClipboardPasteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("clipboard_paste", "Read text from the system clipboard")
    }

    async fn execute(&self, _params: Value) -> Result<ToolResult> {
        #[cfg(target_os = "windows")]
        {
            let output = Command::new("powershell")
                .args(["-Command", "Get-Clipboard"])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    Ok(ToolResult::success_with_data(
                        text.clone(),
                        serde_json::json!({
                            "text": text,
                            "length": text.len()
                        }),
                    ))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "Failed to paste: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to execute: {}", e))),
            }
        }

        #[cfg(target_os = "linux")]
        {
            let output = Command::new("xclip")
                .args(["-selection", "clipboard", "-o"])
                .output()
                .or_else(|_| Command::new("xsel").args(["--clipboard", "--output"]).output());

            match output {
                Ok(o) if o.status.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).to_string();
                    Ok(ToolResult::success_with_data(
                        text.clone(),
                        serde_json::json!({
                            "text": text,
                            "length": text.len()
                        }),
                    ))
                }
                _ => Ok(ToolResult::error("Failed to paste. Install xclip or xsel.")),
            }
        }

        #[cfg(target_os = "macos")]
        {
            let output = Command::new("pbpaste").output();

            match output {
                Ok(o) if o.status.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).to_string();
                    Ok(ToolResult::success_with_data(
                        text.clone(),
                        serde_json::json!({
                            "text": text,
                            "length": text.len()
                        }),
                    ))
                }
                _ => Ok(ToolResult::error("Failed to paste from clipboard")),
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult::error("Clipboard not supported on this platform"))
        }
    }
}

/// Tool to show system notification
pub struct NotificationTool;

#[async_trait]
impl Tool for NotificationTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("show_notification", "Show a system notification")
            .with_param(ToolParameter::string("title", "Notification title", true))
            .with_param(ToolParameter::string("message", "Notification message", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let title = params
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Title is required"))?;

        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Message is required"))?;

        #[cfg(target_os = "windows")]
        {
            let script = format!(
                r#"
                [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
                [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
                $template = @"
                <toast>
                    <visual>
                        <binding template="ToastText02">
                            <text id="1">{}</text>
                            <text id="2">{}</text>
                        </binding>
                    </visual>
                </toast>
"@
                $xml = New-Object Windows.Data.Xml.Dom.XmlDocument
                $xml.LoadXml($template)
                $toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
                [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("Agenticus").Show($toast)
                "#,
                title.replace('"', "'"),
                message.replace('"', "'")
            );

            // Fallback to simpler PowerShell notification
            let simple_script = format!(
                r#"
                Add-Type -AssemblyName System.Windows.Forms
                $notification = New-Object System.Windows.Forms.NotifyIcon
                $notification.Icon = [System.Drawing.SystemIcons]::Information
                $notification.BalloonTipTitle = "{}"
                $notification.BalloonTipText = "{}"
                $notification.Visible = $true
                $notification.ShowBalloonTip(5000)
                Start-Sleep -Seconds 5
                $notification.Dispose()
                "#,
                title.replace('"', "'"),
                message.replace('"', "'")
            );

            let output = Command::new("powershell")
                .args(["-Command", &simple_script])
                .output();

            match output {
                Ok(_) => Ok(ToolResult::success(format!("Notification shown: {}", title))),
                Err(e) => Ok(ToolResult::error(format!("Failed to show notification: {}", e))),
            }
        }

        #[cfg(target_os = "linux")]
        {
            let output = Command::new("notify-send")
                .args([title, message])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Notification shown: {}", title)))
                }
                _ => Ok(ToolResult::error("Failed to show notification. Install libnotify.")),
            }
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                message.replace('"', "\\\""),
                title.replace('"', "\\\"")
            );
            let output = Command::new("osascript")
                .args(["-e", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Notification shown: {}", title)))
                }
                _ => Ok(ToolResult::error("Failed to show notification")),
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            Ok(ToolResult::error("Notifications not supported on this platform"))
        }
    }
}

/// Tool to download a file from URL
pub struct DownloadFileTool;

#[async_trait]
impl Tool for DownloadFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("download_file", "Download a file from a URL and save it locally")
            .with_param(ToolParameter::string("url", "URL of the file to download", true))
            .with_param(ToolParameter::string(
                "destination",
                "Local path to save the file (optional, will auto-name if not provided)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let destination = params.get("destination").and_then(|v| v.as_str());

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(Duration::from_secs(300))
            .build()?;

        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Ok(ToolResult::error(format!(
                "Download failed: HTTP {}",
                response.status()
            )));
        }

        // Get filename from URL or Content-Disposition header
        let filename = destination.map(String::from).unwrap_or_else(|| {
            response
                .headers()
                .get("content-disposition")
                .and_then(|h| h.to_str().ok())
                .and_then(|cd| {
                    cd.split("filename=")
                        .nth(1)
                        .map(|f| f.trim_matches('"').to_string())
                })
                .unwrap_or_else(|| {
                    url.rsplit('/')
                        .next()
                        .unwrap_or("downloaded_file")
                        .split('?')
                        .next()
                        .unwrap_or("downloaded_file")
                        .to_string()
                })
        });

        let content = response.bytes().await?;
        let bytes_written = content.len();

        if let Err(e) = std::fs::write(&filename, &content) {
            return Ok(ToolResult::error(format!("Failed to save file: {}", e)));
        }

        Ok(ToolResult::success_with_data(
            format!(
                "Downloaded {} bytes to {}",
                bytes_written, filename
            ),
            serde_json::json!({
                "url": url,
                "destination": filename,
                "bytes": bytes_written
            }),
        ))
    }
}

/// Tool to wait/sleep
pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("sleep", "Wait for a specified duration")
            .with_param(ToolParameter::number(
                "seconds",
                "Number of seconds to wait",
                true,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let seconds = params
            .get("seconds")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Seconds is required"))?;

        if seconds < 0.0 || seconds > 300.0 {
            return Ok(ToolResult::error("Seconds must be between 0 and 300"));
        }

        tokio::time::sleep(Duration::from_secs_f64(seconds)).await;

        Ok(ToolResult::success(format!("Waited {} seconds", seconds)))
    }
}

/// Tool to get environment variable
pub struct GetEnvTool;

#[async_trait]
impl Tool for GetEnvTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("get_env", "Get the value of an environment variable")
            .with_param(ToolParameter::string("name", "Name of the environment variable", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Name is required"))?;

        match std::env::var(name) {
            Ok(value) => Ok(ToolResult::success_with_data(
                value.clone(),
                serde_json::json!({
                    "name": name,
                    "value": value
                }),
            )),
            Err(_) => Ok(ToolResult::error(format!(
                "Environment variable '{}' not found",
                name
            ))),
        }
    }
}

/// Tool to list environment variables
pub struct ListEnvTool;

#[async_trait]
impl Tool for ListEnvTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("list_env", "List all environment variables")
            .with_param(ToolParameter::string(
                "filter",
                "Filter variables by name prefix (optional)",
                false,
            ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let filter = params.get("filter").and_then(|v| v.as_str());

        let mut vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| {
                filter.map(|f| k.to_uppercase().starts_with(&f.to_uppercase())).unwrap_or(true)
            })
            .collect();

        vars.sort_by(|a, b| a.0.cmp(&b.0));

        let output: String = vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "count": vars.len(),
                "variables": vars.iter().map(|(k, v)| serde_json::json!({"name": k, "value": v})).collect::<Vec<_>>()
            }),
        ))
    }
}

/// Tool to run a shell command
pub struct RunCommandTool;

#[async_trait]
impl Tool for RunCommandTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("run_command", "Run a shell command and return the output")
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

        #[cfg(target_os = "windows")]
        let output = {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", command]);
            if let Some(dir) = working_dir {
                cmd.current_dir(dir);
            }
            cmd.output()
        };

        #[cfg(not(target_os = "windows"))]
        let output = {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", command]);
            if let Some(dir) = working_dir {
                cmd.current_dir(dir);
            }
            cmd.output()
        };

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let exit_code = o.status.code().unwrap_or(-1);

                let output_text = if stderr.is_empty() {
                    stdout.clone()
                } else if stdout.is_empty() {
                    stderr.clone()
                } else {
                    format!("{}\n{}", stdout, stderr)
                };

                if o.status.success() {
                    Ok(ToolResult::success_with_data(
                        output_text,
                        serde_json::json!({
                            "command": command,
                            "exit_code": exit_code,
                            "stdout": stdout,
                            "stderr": stderr
                        }),
                    ))
                } else {
                    Ok(ToolResult::error(format!(
                        "Command failed (exit code {}): {}",
                        exit_code, output_text
                    )))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to execute command: {}", e))),
        }
    }
}

/// Tool to open a file with default application
pub struct OpenWithDefaultTool;

#[async_trait]
impl Tool for OpenWithDefaultTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "open_with_default",
            "Open a file or URL with the system's default application",
        )
        .with_param(ToolParameter::string(
            "path",
            "Path to the file or URL to open",
            true,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Path is required"))?;

        #[cfg(target_os = "windows")]
        let result = Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn();

        #[cfg(target_os = "linux")]
        let result = Command::new("xdg-open").arg(path).spawn();

        #[cfg(target_os = "macos")]
        let result = Command::new("open").arg(path).spawn();

        match result {
            Ok(_) => Ok(ToolResult::success(format!("Opened: {}", path))),
            Err(e) => Ok(ToolResult::error(format!("Failed to open: {}", e))),
        }
    }
}

pub fn register_utils_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(ClipboardCopyTool);
    registry.add_tool(ClipboardPasteTool);
    registry.add_tool(NotificationTool);
    registry.add_tool(DownloadFileTool);
    registry.add_tool(SleepTool);
    registry.add_tool(GetEnvTool);
    registry.add_tool(ListEnvTool);
    registry.add_tool(RunCommandTool);
    registry.add_tool(OpenWithDefaultTool);
}
