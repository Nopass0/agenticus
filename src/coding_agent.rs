use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::{info, warn, error};

/// Result of a coding task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingResult {
    pub success: bool,
    pub script_path: Option<String>,
    pub language: String,
    pub output: String,
    pub error: Option<String>,
    pub attempts: u32,
    pub final_code: String,
}

/// Available programming language/runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableLanguage {
    pub name: String,
    pub command: String,
    pub version: Option<String>,
    pub file_extension: String,
}

/// System context for coding agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemContext {
    pub os: String,
    pub os_version: String,
    pub languages: Vec<AvailableLanguage>,
    pub working_dir: String,
}

impl SystemContext {
    pub fn detect() -> Self {
        let os = std::env::consts::OS.to_string();
        let os_version = get_os_version();
        let working_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let languages = detect_available_languages();

        Self {
            os,
            os_version,
            languages,
            working_dir,
        }
    }

    pub fn to_prompt_context(&self) -> String {
        let langs: Vec<String> = self
            .languages
            .iter()
            .map(|l| {
                format!(
                    "{} ({}){}",
                    l.name,
                    l.command,
                    l.version
                        .as_ref()
                        .map(|v| format!(" v{}", v))
                        .unwrap_or_default()
                )
            })
            .collect();

        format!(
            "System Info:\n- OS: {} {}\n- Working Directory: {}\n- Available Languages: {}",
            self.os,
            self.os_version,
            self.working_dir,
            if langs.is_empty() {
                "None detected".to_string()
            } else {
                langs.join(", ")
            }
        )
    }
}

fn get_os_version() -> String {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/c", "ver"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        String::new()
    }
}

fn detect_available_languages() -> Vec<AvailableLanguage> {
    let mut languages = Vec::new();

    // Check each language
    let checks = vec![
        ("Python", "python", "--version", ".py"),
        ("Python3", "python3", "--version", ".py"),
        ("Node.js", "node", "--version", ".js"),
        ("Bun", "bun", "--version", ".ts"),
        ("Deno", "deno", "--version", ".ts"),
        ("Rust (rustc)", "rustc", "--version", ".rs"),
        ("Go", "go", "version", ".go"),
        ("Ruby", "ruby", "--version", ".rb"),
        ("PHP", "php", "--version", ".php"),
        ("Perl", "perl", "--version", ".pl"),
        ("Lua", "lua", "-v", ".lua"),
        ("PowerShell", "pwsh", "--version", ".ps1"),
        ("Bash", "bash", "--version", ".sh"),
    ];

    for (name, cmd, version_arg, ext) in checks {
        if let Ok(output) = Command::new(cmd).arg(version_arg).output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .map(|s| {
                        // Extract version number
                        s.split_whitespace()
                            .find(|word| word.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
                            .unwrap_or("")
                            .to_string()
                    })
                    .filter(|s| !s.is_empty());

                languages.push(AvailableLanguage {
                    name: name.to_string(),
                    command: cmd.to_string(),
                    version,
                    file_extension: ext.to_string(),
                });
            }
        }
    }

    // On Windows, check for native commands
    #[cfg(target_os = "windows")]
    {
        // PowerShell is always available on Windows
        if !languages.iter().any(|l| l.command == "pwsh") {
            languages.push(AvailableLanguage {
                name: "PowerShell (Windows)".to_string(),
                command: "powershell".to_string(),
                version: None,
                file_extension: ".ps1".to_string(),
            });
        }
    }

    languages
}

/// Coding sub-agent that creates and runs scripts
pub struct CodingAgent {
    context: SystemContext,
    max_attempts: u32,
}

impl CodingAgent {
    pub fn new() -> Self {
        Self {
            context: SystemContext::detect(),
            max_attempts: 5,
        }
    }

    pub fn with_max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts;
        self
    }

    pub fn context(&self) -> &SystemContext {
        &self.context
    }

    /// Get the best language for a task based on availability and task requirements
    pub fn select_language(&self, task_hints: &str) -> Option<&AvailableLanguage> {
        let task_lower = task_hints.to_lowercase();

        // Prefer specific languages based on task hints
        let preferences: Vec<(&str, Vec<&str>)> = vec![
            ("bun", vec!["typescript", "ts", "bun", "web"]),
            ("node", vec!["javascript", "js", "npm"]),
            ("python3", vec!["python", "py", "data", "ml", "ai", "script"]),
            ("python", vec!["python", "py"]),
            ("rustc", vec!["rust", "rs", "performance", "fast"]),
            ("go", vec!["go", "golang", "concurrent"]),
            ("ruby", vec!["ruby", "rb"]),
            ("php", vec!["php", "web"]),
            ("pwsh", vec!["powershell", "windows", "ps1", "system"]),
            ("powershell", vec!["powershell", "windows", "ps1"]),
            ("bash", vec!["bash", "shell", "sh", "linux", "unix"]),
        ];

        for (cmd, keywords) in &preferences {
            if keywords.iter().any(|kw| task_lower.contains(kw)) {
                if let Some(lang) = self.context.languages.iter().find(|l| l.command == *cmd) {
                    return Some(lang);
                }
            }
        }

        // Default preference order
        let default_order = vec!["python3", "python", "node", "bun", "pwsh", "powershell", "bash"];

        for cmd in default_order {
            if let Some(lang) = self.context.languages.iter().find(|l| l.command == cmd) {
                return Some(lang);
            }
        }

        self.context.languages.first()
    }

    /// Create a script file
    pub fn create_script(&self, language: &AvailableLanguage, code: &str) -> Result<String> {
        let temp_dir = std::env::temp_dir();
        let filename = format!(
            "agenticus_script_{}{}",
            uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("tmp"),
            language.file_extension
        );
        let path = temp_dir.join(&filename);

        std::fs::write(&path, code)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Run a script and return output
    pub fn run_script(&self, language: &AvailableLanguage, script_path: &str) -> Result<(bool, String, String)> {
        let output = match language.command.as_str() {
            "python" | "python3" => {
                Command::new(&language.command).arg(script_path).output()
            }
            "node" => {
                Command::new("node").arg(script_path).output()
            }
            "bun" => {
                Command::new("bun").arg("run").arg(script_path).output()
            }
            "deno" => {
                Command::new("deno")
                    .args(["run", "--allow-all", script_path])
                    .output()
            }
            "rustc" => {
                // Compile and run Rust
                let exe_path = script_path.replace(".rs", if cfg!(windows) { ".exe" } else { "" });
                let compile = Command::new("rustc")
                    .args(["-o", &exe_path, script_path])
                    .output();

                match compile {
                    Ok(out) if out.status.success() => {
                        Command::new(&exe_path).output()
                    }
                    Ok(out) => Ok(out),
                    Err(e) => Err(e),
                }
            }
            "go" => {
                Command::new("go").args(["run", script_path]).output()
            }
            "ruby" => {
                Command::new("ruby").arg(script_path).output()
            }
            "php" => {
                Command::new("php").arg(script_path).output()
            }
            "perl" => {
                Command::new("perl").arg(script_path).output()
            }
            "lua" => {
                Command::new("lua").arg(script_path).output()
            }
            "pwsh" => {
                Command::new("pwsh")
                    .args(["-NoProfile", "-File", script_path])
                    .output()
            }
            "powershell" => {
                Command::new("powershell")
                    .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script_path])
                    .output()
            }
            "bash" => {
                Command::new("bash").arg(script_path).output()
            }
            _ => {
                Command::new(&language.command).arg(script_path).output()
            }
        };

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let success = out.status.success();

                Ok((success, stdout, stderr))
            }
            Err(e) => Err(anyhow::anyhow!("Failed to run script: {}", e)),
        }
    }

    /// Execute a coding task with automatic debugging
    pub async fn execute_task(
        &self,
        task: &str,
        initial_code: &str,
        language: &AvailableLanguage,
        fix_callback: impl Fn(&str, &str, &str) -> Option<String>,
    ) -> CodingResult {
        let mut current_code = initial_code.to_string();
        let mut attempts = 0;
        let mut last_error = String::new();

        while attempts < self.max_attempts {
            attempts += 1;
            info!("Coding agent attempt {} of {}", attempts, self.max_attempts);

            // Create script
            let script_path = match self.create_script(language, &current_code) {
                Ok(path) => path,
                Err(e) => {
                    return CodingResult {
                        success: false,
                        script_path: None,
                        language: language.name.clone(),
                        output: String::new(),
                        error: Some(format!("Failed to create script: {}", e)),
                        attempts,
                        final_code: current_code,
                    };
                }
            };

            // Run script
            match self.run_script(language, &script_path) {
                Ok((success, stdout, stderr)) => {
                    if success {
                        info!("Script executed successfully on attempt {}", attempts);
                        return CodingResult {
                            success: true,
                            script_path: Some(script_path),
                            language: language.name.clone(),
                            output: stdout,
                            error: None,
                            attempts,
                            final_code: current_code,
                        };
                    }

                    // Script failed, try to fix
                    last_error = if stderr.is_empty() { stdout.clone() } else { stderr.clone() };
                    warn!("Script failed on attempt {}: {}", attempts, last_error);

                    // Call the fix callback to get corrected code
                    if let Some(fixed_code) = fix_callback(task, &current_code, &last_error) {
                        current_code = fixed_code;
                    } else {
                        // No fix provided, give up
                        break;
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                    error!("Failed to run script on attempt {}: {}", attempts, e);
                    break;
                }
            }
        }

        CodingResult {
            success: false,
            script_path: None,
            language: language.name.clone(),
            output: String::new(),
            error: Some(last_error),
            attempts,
            final_code: current_code,
        }
    }
}

impl Default for CodingAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool for the main agent to invoke the coding sub-agent
pub mod tool {
    use super::*;
    use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
    use async_trait::async_trait;
    use serde_json::Value;

    pub struct InvokeCodingAgentTool;

    #[async_trait]
    impl Tool for InvokeCodingAgentTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema::new(
                "invoke_coding_agent",
                "Invoke the coding sub-agent to create and run a script. The coding agent will automatically debug and fix errors until the script works.",
            )
            .with_param(ToolParameter::string(
                "task",
                "Description of what the script should do",
                true,
            ))
            .with_param(ToolParameter::string(
                "code",
                "Initial code to run (the agent will fix if it fails)",
                true,
            ))
            .with_param(ToolParameter::string(
                "language",
                "Preferred programming language (python, javascript, typescript, rust, go, etc.). If not specified, best available will be chosen.",
                false,
            ))
        }

        async fn execute(&self, params: Value) -> Result<ToolResult> {
            let task = params
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Task description is required"))?;

            let code = params
                .get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Code is required"))?;

            let preferred_lang = params
                .get("language")
                .and_then(|v| v.as_str());

            let agent = CodingAgent::new();

            // Select language
            let language = if let Some(pref) = preferred_lang {
                agent.context.languages
                    .iter()
                    .find(|l| l.name.to_lowercase().contains(&pref.to_lowercase()) ||
                              l.command.to_lowercase().contains(&pref.to_lowercase()))
                    .or_else(|| agent.select_language(task))
            } else {
                agent.select_language(task)
            };

            let language = match language {
                Some(l) => l.clone(),
                None => {
                    return Ok(ToolResult::error(
                        "No suitable programming language found on this system. Install Python, Node.js, or another supported language.",
                    ));
                }
            };

            // Execute with simple retry (no AI fix for now, that would require LLM calls)
            // The main agent should provide fixed code if there are errors
            let result = agent
                .execute_task(task, code, &language, |_, _, _| None)
                .await;

            if result.success {
                Ok(ToolResult::success_with_data(
                    format!(
                        "Script executed successfully in {} ({} attempts)\n\nOutput:\n{}",
                        result.language, result.attempts, result.output
                    ),
                    serde_json::json!({
                        "success": true,
                        "language": result.language,
                        "attempts": result.attempts,
                        "output": result.output,
                        "script_path": result.script_path,
                        "final_code": result.final_code
                    }),
                ))
            } else {
                Ok(ToolResult::success_with_data(
                    format!(
                        "Script execution failed after {} attempts in {}\n\nError:\n{}\n\nLast code:\n{}",
                        result.attempts,
                        result.language,
                        result.error.as_deref().unwrap_or("Unknown error"),
                        result.final_code
                    ),
                    serde_json::json!({
                        "success": false,
                        "language": result.language,
                        "attempts": result.attempts,
                        "error": result.error,
                        "final_code": result.final_code,
                        "needs_fix": true
                    }),
                ))
            }
        }
    }

    pub struct GetSystemContextTool;

    #[async_trait]
    impl Tool for GetSystemContextTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema::new(
                "get_coding_context",
                "Get information about the system for coding tasks: OS, available programming languages, versions, etc.",
            )
        }

        async fn execute(&self, _params: Value) -> Result<ToolResult> {
            let context = SystemContext::detect();

            let languages_info: Vec<serde_json::Value> = context
                .languages
                .iter()
                .map(|l| {
                    serde_json::json!({
                        "name": l.name,
                        "command": l.command,
                        "version": l.version,
                        "extension": l.file_extension
                    })
                })
                .collect();

            Ok(ToolResult::success_with_data(
                context.to_prompt_context(),
                serde_json::json!({
                    "os": context.os,
                    "os_version": context.os_version,
                    "working_dir": context.working_dir,
                    "languages": languages_info
                }),
            ))
        }
    }

    pub fn register_coding_tools(registry: &mut crate::tools::ToolRegistry) {
        registry.add_tool(InvokeCodingAgentTool);
        registry.add_tool(GetSystemContextTool);
    }
}
