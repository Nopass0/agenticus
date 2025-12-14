mod agent;
mod cli;
mod config;
mod llm;
mod logging;
mod tools;

use agent::Agent;
use anyhow::Result;
use cli::*;
use config::Config;
use llm::{OllamaProvider, OpenRouterProvider};
use logging::InteractionLogger;
use std::sync::Arc;
use tools::memory::{create_shared_memory, register_memory_tools};
use tools::ToolRegistry;
use tracing::info;

fn create_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    // Register all tools
    tools::system::register_system_tools(&mut registry);
    tools::browser::register_browser_tools(&mut registry);
    tools::search::register_search_tools(&mut registry);
    tools::api::register_api_tools(&mut registry);
    tools::processes::register_process_tools(&mut registry);
    tools::screenshot::register_screenshot_tools(&mut registry);

    // Register memory tools with shared memory
    let memory = create_shared_memory();
    register_memory_tools(&mut registry, memory);

    registry
}

fn create_provider(config: &Config) -> Arc<dyn llm::LlmProvider> {
    match config.general.provider.as_str() {
        "ollama" => Arc::new(OllamaProvider::new(
            &config.ollama.url,
            &config.ollama.model,
        )),
        _ => Arc::new(OpenRouterProvider::new(
            &config.openrouter.url,
            &config.openrouter.model,
            &config.openrouter.api_key,
        )),
    }
}

async fn handle_command(
    cmd: &str,
    registry: &ToolRegistry,
    config: &Config,
    memory: &tools::memory::SharedMemory,
) -> bool {
    match cmd {
        "/help" => {
            print_help();
            true
        }
        "/tools" => {
            print_tools(registry);
            true
        }
        "/memory" => {
            let store = memory.read().unwrap();
            if store.entries.is_empty() {
                print_info("No memories stored yet.");
            } else {
                println!("\n{}", "Stored Memories:".bright_yellow().bold());
                println!("{}", "─".repeat(60).dimmed());
                for (key, entry) in &store.entries {
                    use colored::Colorize;
                    let cat = entry
                        .category
                        .as_ref()
                        .map(|c| format!(" [{}]", c))
                        .unwrap_or_default();
                    println!(
                        "  {} {}{}: {}",
                        "●".bright_cyan(),
                        key.bright_white().bold(),
                        cat.dimmed(),
                        entry.value
                    );
                }
                println!();
            }
            true
        }
        "/clear" => {
            clear_screen();
            print_banner();
            true
        }
        "/config" => {
            use colored::Colorize;
            println!("\n{}", "Current Configuration:".bright_yellow().bold());
            println!("{}", "─".repeat(60).dimmed());
            println!("  Provider: {}", config.general.provider.bright_white());
            println!("  Language: {}", config.general.language.bright_white());
            println!("  Max Steps: {}", config.general.max_steps.to_string().bright_white());
            println!();
            println!("  {}", "Ollama:".bright_blue());
            println!("    URL: {}", config.ollama.url.dimmed());
            println!("    Model: {}", config.ollama.model.dimmed());
            println!();
            println!("  {}", "OpenRouter:".bright_blue());
            println!("    URL: {}", config.openrouter.url.dimmed());
            println!("    Model: {}", config.openrouter.model.dimmed());
            println!(
                "    API Key: {}",
                if config.openrouter.api_key.is_empty() {
                    "(not set)".bright_red().to_string()
                } else {
                    format!("{}...", &config.openrouter.api_key[..8.min(config.openrouter.api_key.len())]).dimmed().to_string()
                }
            );
            println!();
            println!(
                "  Config file: {}",
                Config::config_path().to_string_lossy().dimmed()
            );
            println!();
            true
        }
        "/exit" | "/quit" | "/q" => {
            print_info("Goodbye!");
            false
        }
        _ => {
            print_warning(&format!("Unknown command: {}. Type /help for help.", cmd));
            true
        }
    }
}

use colored::Colorize;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config = Config::load()?;

    // Initialize logging
    let _guard = logging::init_logging(&config)?;
    info!("Agenticus starting up");

    // Create interaction logger
    let interaction_logger = InteractionLogger::new(&config.logging.log_dir)?;

    // Create components
    let registry = create_tool_registry();
    let memory = create_shared_memory();

    // Validate API key if using OpenRouter
    if config.general.provider == "openrouter" && config.openrouter.api_key.is_empty() {
        print_banner();
        print_error("OpenRouter API key not configured!");
        println!();
        println!(
            "Please set your API key in the config file:\n  {}",
            Config::config_path().to_string_lossy().bright_cyan()
        );
        println!();
        println!("Or set the OPENROUTER_API_KEY environment variable.");
        println!();

        // Check environment variable
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
            if !key.is_empty() {
                print_info("Found OPENROUTER_API_KEY in environment, using that.");
                let mut new_config = config.clone();
                new_config.openrouter.api_key = key;
                return run_chat_loop(new_config, registry, memory, interaction_logger).await;
            }
        }

        return Ok(());
    }

    run_chat_loop(config, registry, memory, interaction_logger).await
}

async fn run_chat_loop(
    config: Config,
    registry: ToolRegistry,
    memory: tools::memory::SharedMemory,
    interaction_logger: InteractionLogger,
) -> Result<()> {
    let provider = create_provider(&config);

    // Print welcome
    clear_screen();
    print_banner();
    print_config_info(
        provider.name(),
        provider.model(),
        &config.general.language,
    );

    // Create agent with step callback
    let registry_arc = Arc::new(registry);
    let registry_for_agent = registry_arc.clone();

    let agent = Agent::new(provider, registry_for_agent, config.clone())
        .with_step_callback(Box::new(|step| {
            print_step(step);
        }));

    // Main chat loop
    loop {
        let input = match read_input() {
            Some(s) if !s.is_empty() => s,
            Some(_) => continue,
            None => break,
        };

        // Handle commands
        if input.starts_with('/') {
            if !handle_command(&input, &registry_arc, &config, &memory).await {
                break;
            }
            continue;
        }

        // Process user query
        print_user_message(&input);
        print_thinking();

        let start_time = std::time::Instant::now();

        match agent.run(&input).await {
            Ok(log) => {
                clear_thinking();

                // Print final response
                if let Some(response) = &log.final_response {
                    print_response(response);
                } else {
                    print_warning("No response generated.");
                }

                // Print summary
                let duration = start_time.elapsed().as_secs_f64();
                println!(
                    "{} {} steps in {}",
                    "Completed:".dimmed(),
                    log.total_steps.to_string().bright_white(),
                    format_duration(duration).bright_cyan()
                );

                // Log the interaction
                if let Err(e) = interaction_logger.log_interaction(&log) {
                    tracing::warn!("Failed to log interaction: {}", e);
                }
            }
            Err(e) => {
                clear_thinking();
                print_error(&format!("{}", e));

                // Check for common errors
                if e.to_string().contains("API") || e.to_string().contains("401") {
                    print_info("Check your API key configuration.");
                } else if e.to_string().contains("connection") {
                    print_info("Check your network connection and provider URL.");
                }
            }
        }

        println!();
    }

    print_success("Session ended. Logs saved.");
    Ok(())
}
