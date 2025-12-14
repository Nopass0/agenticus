use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use sysinfo::{System, Pid};
use std::process::Command;
use std::collections::HashMap;

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
            let display_name = if name.chars().count() > 20 {
                format!("{}...", name.chars().take(17).collect::<String>())
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

impl LaunchAppTool {
    /// Get common app name aliases (especially for Windows)
    fn get_app_aliases() -> HashMap<&'static str, Vec<&'static str>> {
        let mut aliases = HashMap::new();

        // Games
        aliases.insert("minesweeper", vec![
            "microsoft.minesweeper",
            "minesweeper",
            "сапер",
            "winmine",
        ]);
        aliases.insert("сапер", vec![
            "microsoft.minesweeper",
            "minesweeper",
        ]);

        // Common apps
        aliases.insert("steam", vec![
            "steam",
            "C:\\Program Files (x86)\\Steam\\steam.exe",
            "C:\\Program Files\\Steam\\steam.exe",
        ]);
        aliases.insert("стим", vec![
            "steam",
            "C:\\Program Files (x86)\\Steam\\steam.exe",
        ]);

        aliases.insert("chrome", vec![
            "chrome",
            "google-chrome",
            "google-chrome-stable",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        ]);

        aliases.insert("firefox", vec![
            "firefox",
            "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
            "C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe",
        ]);

        aliases.insert("notepad", vec![
            "notepad",
            "notepad.exe",
        ]);

        aliases.insert("calculator", vec![
            "calc",
            "calc.exe",
            "microsoft.windowscalculator",
            "калькулятор",
        ]);
        aliases.insert("калькулятор", vec![
            "calc",
            "microsoft.windowscalculator",
        ]);

        aliases.insert("vscode", vec![
            "code",
            "C:\\Program Files\\Microsoft VS Code\\Code.exe",
            "C:\\Users\\*\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe",
        ]);

        aliases.insert("telegram", vec![
            "telegram",
            "C:\\Users\\*\\AppData\\Roaming\\Telegram Desktop\\Telegram.exe",
        ]);

        aliases.insert("discord", vec![
            "discord",
            "C:\\Users\\*\\AppData\\Local\\Discord\\Update.exe --processStart Discord.exe",
        ]);

        aliases.insert("spotify", vec![
            "spotify",
            "C:\\Users\\*\\AppData\\Roaming\\Spotify\\Spotify.exe",
        ]);

        aliases
    }

    #[cfg(target_os = "windows")]
    fn find_windows_app(app_name: &str) -> Option<String> {
        use std::path::Path;

        let app_lower = app_name.to_lowercase();

        // Check aliases first
        let aliases = Self::get_app_aliases();
        let names_to_try: Vec<String> = if let Some(alias_list) = aliases.get(app_lower.as_str()) {
            alias_list.iter().map(|s| s.to_string()).collect()
        } else {
            vec![app_name.to_string()]
        };

        // Get username for path expansion
        let username = std::env::var("USERNAME").unwrap_or_default();

        for name in &names_to_try {
            // Replace * with username in paths
            let expanded_name = name.replace("*", &username);

            // Check if it's a direct path that exists
            if Path::new(&expanded_name).exists() {
                return Some(expanded_name);
            }

            // Check common Windows paths
            let common_paths = vec![
                format!("C:\\Program Files\\{}\\{}.exe", expanded_name, expanded_name),
                format!("C:\\Program Files (x86)\\{}\\{}.exe", expanded_name, expanded_name),
                format!("C:\\Users\\{}\\AppData\\Local\\Programs\\{}\\{}.exe", username, expanded_name, expanded_name),
                format!("C:\\Users\\{}\\AppData\\Roaming\\{}\\{}.exe", username, expanded_name, expanded_name),
            ];

            for path in common_paths {
                if Path::new(&path).exists() {
                    return Some(path);
                }
            }
        }

        None
    }

    #[cfg(target_os = "windows")]
    fn try_launch_windows_store_app(app_name: &str) -> Result<ToolResult> {
        // Try to launch as Windows Store app using shell:AppsFolder
        let app_lower = app_name.to_lowercase();

        // Map common names to Windows Store app IDs
        let store_apps: HashMap<&str, &str> = [
            ("minesweeper", "Microsoft.MicrosoftMinesweeper_8wekyb3d8bbwe!App"),
            ("сапер", "Microsoft.MicrosoftMinesweeper_8wekyb3d8bbwe!App"),
            ("calculator", "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            ("калькулятор", "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            ("photos", "Microsoft.Windows.Photos_8wekyb3d8bbwe!App"),
            ("фотографии", "Microsoft.Windows.Photos_8wekyb3d8bbwe!App"),
            ("store", "Microsoft.WindowsStore_8wekyb3d8bbwe!App"),
            ("магазин", "Microsoft.WindowsStore_8wekyb3d8bbwe!App"),
            ("mail", "microsoft.windowscommunicationsapps_8wekyb3d8bbwe!microsoft.windowslive.mail"),
            ("почта", "microsoft.windowscommunicationsapps_8wekyb3d8bbwe!microsoft.windowslive.mail"),
            ("solitaire", "Microsoft.MicrosoftSolitaireCollection_8wekyb3d8bbwe!App"),
            ("солитер", "Microsoft.MicrosoftSolitaireCollection_8wekyb3d8bbwe!App"),
            ("пасьянс", "Microsoft.MicrosoftSolitaireCollection_8wekyb3d8bbwe!App"),
        ].into_iter().collect();

        let app_id = store_apps.get(app_lower.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // Try to construct app ID from name
                format!("Microsoft.{}!App", app_name)
            });

        // Use PowerShell to launch store app
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Start-Process 'shell:AppsFolder\\{}'",
                    app_id
                ),
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                Ok(ToolResult::success_with_data(
                    format!("Launched Windows Store app: {}", app_name),
                    serde_json::json!({
                        "app": app_name,
                        "type": "windows_store",
                        "app_id": app_id
                    }),
                ))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(anyhow::anyhow!("Failed to launch store app: {}", stderr))
            }
            Err(e) => Err(anyhow::anyhow!("PowerShell error: {}", e)),
        }
    }

    #[cfg(target_os = "windows")]
    fn try_powershell_start(app_name: &str, args: &[String]) -> Result<ToolResult> {
        // Use PowerShell Start-Process which has better app resolution
        let args_str = args.join(" ");
        let cmd = if args_str.is_empty() {
            format!("Start-Process '{}'", app_name)
        } else {
            format!("Start-Process '{}' -ArgumentList '{}'", app_name, args_str)
        };

        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", &cmd])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                Ok(ToolResult::success_with_data(
                    format!("Launched '{}' via PowerShell", app_name),
                    serde_json::json!({
                        "app": app_name,
                        "method": "powershell_start_process",
                        "args": args
                    }),
                ))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(anyhow::anyhow!("PowerShell Start-Process failed: {}", stderr))
            }
            Err(e) => Err(anyhow::anyhow!("PowerShell error: {}", e)),
        }
    }

    #[cfg(target_os = "windows")]
    fn search_installed_apps(app_name: &str) -> Option<String> {
        // Search in registry for installed applications
        let search_query = format!(
            r#"
            $apps = @()
            $paths = @(
                'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\*',
                'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
            )
            foreach ($path in $paths) {{
                Get-ItemProperty $path 2>$null | Where-Object {{
                    $_.DisplayName -like '*{0}*' -or $_.PSChildName -like '*{0}*'
                }} | ForEach-Object {{
                    if ($_.'(default)') {{ $apps += $_.'(default)' }}
                    elseif ($_.InstallLocation) {{ $apps += "$($_.InstallLocation)\*.exe" }}
                }}
            }}
            $apps | Select-Object -First 1
            "#,
            app_name
        );

        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", &search_query])
            .output();

        if let Ok(out) = output {
            let result = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !result.is_empty() && std::path::Path::new(&result).exists() {
                return Some(result);
            }
        }

        None
    }
}

#[async_trait]
impl Tool for LaunchAppTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "launch_app",
            "Launch an application or executable by name or path. Supports Windows Store apps, Steam, browsers, and common applications.",
        )
        .with_param(ToolParameter::string(
            "app",
            "Application name (e.g., 'steam', 'minesweeper', 'chrome', 'notepad') or full path to executable",
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

        let _detached = params
            .get("detached")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        #[cfg(target_os = "windows")]
        {
            // Step 1: Check if it's a direct path
            if std::path::Path::new(app).exists() {
                let mut cmd = Command::new(app);
                cmd.args(&args);
                match cmd.spawn() {
                    Ok(child) => return Ok(ToolResult::success_with_data(
                        format!("Launched '{}' with PID {}", app, child.id()),
                        serde_json::json!({
                            "app": app,
                            "pid": child.id(),
                            "args": args
                        }),
                    )),
                    Err(e) => return Ok(ToolResult::error(format!("Failed to launch '{}': {}", app, e))),
                }
            }

            // Step 2: Try to find in common paths and aliases
            if let Some(path) = Self::find_windows_app(app) {
                let mut cmd = Command::new(&path);
                cmd.args(&args);
                match cmd.spawn() {
                    Ok(child) => return Ok(ToolResult::success_with_data(
                        format!("Launched '{}' (found at: {}) with PID {}", app, path, child.id()),
                        serde_json::json!({
                            "app": app,
                            "pid": child.id(),
                            "path": path,
                            "args": args
                        }),
                    )),
                    Err(_) => {} // Continue to next method
                }
            }

            // Step 3: Try Windows Store app launch
            if let Ok(result) = Self::try_launch_windows_store_app(app) {
                if result.success {
                    return Ok(result);
                }
            }

            // Step 4: Search registry for installed apps
            if let Some(path) = Self::search_installed_apps(app) {
                let mut cmd = Command::new(&path);
                cmd.args(&args);
                match cmd.spawn() {
                    Ok(child) => return Ok(ToolResult::success_with_data(
                        format!("Launched '{}' (registry: {}) with PID {}", app, path, child.id()),
                        serde_json::json!({
                            "app": app,
                            "pid": child.id(),
                            "path": path,
                            "args": args,
                            "method": "registry_search"
                        }),
                    )),
                    Err(_) => {} // Continue to next method
                }
            }

            // Step 5: Try PowerShell Start-Process as final fallback
            if let Ok(result) = Self::try_powershell_start(app, &args) {
                if result.success {
                    return Ok(result);
                }
            }

            // Step 6: Try which/PATH lookup as final fallback
            if let Ok(path) = which::which(app) {
                let mut cmd = Command::new(&path);
                cmd.args(&args);
                match cmd.spawn() {
                    Ok(child) => return Ok(ToolResult::success_with_data(
                        format!("Launched '{}' with PID {}", app, child.id()),
                        serde_json::json!({
                            "app": app,
                            "pid": child.id(),
                            "path": path.to_string_lossy(),
                            "args": args
                        }),
                    )),
                    Err(e) => return Ok(ToolResult::error(format!("Failed to launch '{}': {}", app, e))),
                }
            }

            // Nothing worked
            return Ok(ToolResult::error(format!(
                "Could not find or launch application '{}'. Tried: direct path, common locations, Windows Store apps, registry search, PowerShell, and PATH lookup.",
                app
            )));
        }

        #[cfg(not(target_os = "windows"))]
        {
            // Linux/macOS - use which and direct execution
            let app_path = if std::path::Path::new(app).exists() {
                app.to_string()
            } else {
                which::which(app)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| app.to_string())
            };

            let mut cmd = Command::new(&app_path);
            cmd.args(&args);

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
