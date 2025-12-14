use crate::agent::{Agent, InteractionLog, ReasoningStep};
use colored::*;
use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

/// Print the welcome banner
pub fn print_banner() {
    let banner = r#"
    ╔═══════════════════════════════════════════════════════════════╗
    ║                                                               ║
    ║     ▄▀█ █▀▀ █▀▀ █▄ █ ▀█▀ █ █▀▀ █ █ █▀                        ║
    ║     █▀█ █▄█ ██▄ █ ▀█  █  █ █▄▄ █▄█ ▄█                        ║
    ║                                                               ║
    ║     AI Agent with Tool-Calling Capabilities                   ║
    ║                                                               ║
    ╚═══════════════════════════════════════════════════════════════╝
    "#;

    println!("{}", banner.bright_cyan());
}

/// Print configuration info
pub fn print_config_info(provider: &str, model: &str, language: &str) {
    println!(
        "{}",
        "─".repeat(60).dimmed()
    );
    println!(
        "  {} {} ({})",
        "Provider:".bright_blue(),
        provider.bright_white(),
        model.dimmed()
    );
    println!(
        "  {} {}",
        "Language:".bright_blue(),
        language.bright_white()
    );
    println!(
        "{}",
        "─".repeat(60).dimmed()
    );
    println!();
    println!(
        "  {} {}",
        "Commands:".bright_yellow(),
        "/help, /tools, /memory, /clear, /config, /exit".dimmed()
    );
    println!();
}

/// Print the input prompt
pub fn print_prompt() {
    print!("{} ", "▶".bright_green());
    io::stdout().flush().unwrap();
}

/// Print user message
pub fn print_user_message(msg: &str) {
    println!();
    println!("{} {}", "You:".bright_blue().bold(), msg);
}

/// Print a thinking indicator
pub fn print_thinking() {
    print!(
        "\r{} {}",
        "⟳".bright_yellow(),
        "Thinking...".bright_yellow()
    );
    io::stdout().flush().unwrap();
}

/// Clear the thinking indicator
pub fn clear_thinking() {
    print!("\r{}", " ".repeat(40));
    print!("\r");
    io::stdout().flush().unwrap();
}

/// Print a reasoning step
pub fn print_step(step: &ReasoningStep) {
    println!();

    // Step header
    println!(
        "{} {}",
        format!("Step {}:", step.step_number).bright_magenta().bold(),
        "─".repeat(50).dimmed()
    );

    // Thought (if any)
    if let Some(thought) = &step.thought {
        if !thought.is_empty() {
            println!(
                "  {} {}",
                "💭".dimmed(),
                thought.dimmed()
            );
        }
    }

    // Tool call (if any)
    if let Some(tool_call) = &step.tool_call {
        println!(
            "  {} {} {}",
            "🔧".bright_cyan(),
            "Tool:".bright_cyan(),
            tool_call.name.bright_white().bold()
        );

        // Pretty print arguments
        if let Ok(args_str) = serde_json::to_string_pretty(&tool_call.arguments) {
            for line in args_str.lines() {
                println!("     {}", line.dimmed());
            }
        }
    }

    // Tool result (if any)
    if let Some(result) = &step.tool_result {
        println!(
            "  {} {}",
            "📋".bright_green(),
            "Result:".bright_green()
        );

        // Truncate long results
        let display_result = if result.len() > 500 {
            format!("{}...\n     [truncated]", &result[..500])
        } else {
            result.clone()
        };

        for line in display_result.lines().take(15) {
            println!("     {}", line.dimmed());
        }
    }
}

/// Print the final response
pub fn print_response(response: &str) {
    println!();
    println!(
        "{}",
        "═".repeat(60).bright_cyan()
    );
    println!(
        "{} {}",
        "🤖".bright_cyan(),
        "Agenticus:".bright_cyan().bold()
    );
    println!(
        "{}",
        "═".repeat(60).bright_cyan()
    );
    println!();

    // Print response with proper formatting
    for line in response.lines() {
        if line.starts_with("# ") || line.starts_with("## ") {
            println!("{}", line.bright_white().bold());
        } else if line.starts_with("- ") || line.starts_with("* ") {
            println!("{}", line.bright_white());
        } else if line.starts_with("```") {
            println!("{}", line.bright_yellow());
        } else {
            println!("{}", line);
        }
    }

    println!();
    println!(
        "{}",
        "─".repeat(60).dimmed()
    );
}

/// Print an error message
pub fn print_error(msg: &str) {
    println!();
    println!(
        "{} {} {}",
        "❌".bright_red(),
        "Error:".bright_red().bold(),
        msg.bright_red()
    );
}

/// Print a warning message
pub fn print_warning(msg: &str) {
    println!(
        "{} {} {}",
        "⚠️".bright_yellow(),
        "Warning:".bright_yellow().bold(),
        msg.yellow()
    );
}

/// Print an info message
pub fn print_info(msg: &str) {
    println!(
        "{} {}",
        "ℹ️".bright_blue(),
        msg.bright_blue()
    );
}

/// Print success message
pub fn print_success(msg: &str) {
    println!(
        "{} {}",
        "✓".bright_green(),
        msg.bright_green()
    );
}

/// Print help information
pub fn print_help() {
    println!();
    println!("{}", "Available Commands:".bright_yellow().bold());
    println!();
    println!("  {}  - Show this help message", "/help".bright_cyan());
    println!("  {} - List all available tools", "/tools".bright_cyan());
    println!("  {} - Show memory contents", "/memory".bright_cyan());
    println!("  {} - Clear the screen", "/clear".bright_cyan());
    println!("  {} - Show current configuration", "/config".bright_cyan());
    println!("  {}  - Exit the program", "/exit".bright_cyan());
    println!();
    println!("{}", "Tips:".bright_yellow().bold());
    println!("  • Ask anything - the AI will use tools as needed");
    println!("  • Say 'remember' to save info to long-term memory");
    println!("  • Ask about system, time, web pages, run commands, etc.");
    println!();
}

/// Print tools list
pub fn print_tools(registry: &crate::tools::ToolRegistry) {
    println!();
    println!("{}", "Available Tools:".bright_yellow().bold());
    println!("{}", "─".repeat(60).dimmed());

    for schema in registry.schemas() {
        println!();
        println!(
            "  {} {}",
            "●".bright_cyan(),
            schema.name.bright_white().bold()
        );
        println!("    {}", schema.description.dimmed());

        if !schema.parameters.is_empty() {
            println!("    {}:", "Parameters".bright_blue());
            for param in &schema.parameters {
                let req = if param.required {
                    " (required)".bright_red().to_string()
                } else {
                    " (optional)".dimmed().to_string()
                };
                println!(
                    "      • {}: {} [{}]{}",
                    param.name.bright_white(),
                    param.description.dimmed(),
                    param.param_type.bright_yellow(),
                    req
                );
            }
        }
    }
    println!();
}

/// Clear the terminal screen
pub fn clear_screen() {
    execute!(io::stdout(), terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).unwrap();
}

/// Read a line of input
pub fn read_input() -> Option<String> {
    print_prompt();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => None, // EOF
        Ok(_) => Some(input.trim().to_string()),
        Err(_) => None,
    }
}

/// Print interaction summary
pub fn print_summary(log: &InteractionLog) {
    println!();
    println!(
        "{} {} steps, {}",
        "Summary:".dimmed(),
        log.total_steps.to_string().bright_white(),
        if log.success {
            "completed".bright_green()
        } else {
            "incomplete".bright_red()
        }
    );
}

/// Format duration
pub fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{:.1}s", secs)
    } else {
        format!("{:.0}m {:.0}s", secs / 60.0, secs % 60.0)
    }
}

/// Spinner animation for waiting
pub struct Spinner {
    frames: Vec<&'static str>,
    current: usize,
    message: String,
}

impl Spinner {
    pub fn new(message: &str) -> Self {
        Self {
            frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            current: 0,
            message: message.to_string(),
        }
    }

    pub fn tick(&mut self) {
        self.current = (self.current + 1) % self.frames.len();
        print!(
            "\r{} {}",
            self.frames[self.current].bright_cyan(),
            self.message.bright_yellow()
        );
        io::stdout().flush().unwrap();
    }

    pub fn finish(&self, success: bool) {
        if success {
            println!(
                "\r{} {}",
                "✓".bright_green(),
                self.message.bright_green()
            );
        } else {
            println!(
                "\r{} {}",
                "✗".bright_red(),
                self.message.bright_red()
            );
        }
    }
}
