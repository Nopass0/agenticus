use crate::config::Config;
use crate::llm::{LlmProvider, LlmResponse, Message, ToolCall};
use crate::tools::ToolRegistry;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// A step in the agent's reasoning process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step_number: usize,
    pub timestamp: String,
    pub thought: Option<String>,
    pub tool_call: Option<ToolCallRecord>,
    pub tool_result: Option<String>,
    pub is_final: bool,
}

/// Record of a tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: Value,
}

/// Complete interaction record for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionLog {
    pub id: String,
    pub timestamp: String,
    pub user_query: String,
    pub steps: Vec<ReasoningStep>,
    pub final_response: Option<String>,
    pub total_steps: usize,
    pub success: bool,
}

/// Callback for step updates
pub type StepCallback = Box<dyn Fn(&ReasoningStep) + Send + Sync>;

/// The main AI agent
pub struct Agent {
    provider: Arc<dyn LlmProvider>,
    registry: Arc<ToolRegistry>,
    config: Config,
    step_callback: Option<StepCallback>,
}

impl Agent {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        registry: Arc<ToolRegistry>,
        config: Config,
    ) -> Self {
        Self {
            provider,
            registry,
            config,
            step_callback: None,
        }
    }

    pub fn with_step_callback(mut self, callback: StepCallback) -> Self {
        self.step_callback = Some(callback);
        self
    }

    /// Create the system prompt
    fn system_prompt(&self) -> String {
        let tools_desc = self.registry.format_for_prompt();
        let language = &self.config.general.language;

        format!(
            r#"You are Agenticus, a powerful AI assistant with access to various tools.
Your responses should be in {language}.

IMPORTANT: You MUST use tools to answer questions. DO NOT ask for permission - just use the tools directly!

When the user asks a question that requires information (like processes, time, system info, web search, etc.), IMMEDIATELY call the appropriate tool.

{tools_desc}

## CRITICAL RULES

1. **ALWAYS USE TOOLS** - When a user asks about processes, system info, time, web content, etc., call the tool IMMEDIATELY. DO NOT ask "do you want me to...?" - just do it!

2. **BE PROACTIVE** - If the user asks "what processes are running?", call list_processes right away. If they ask "what time is it?", call get_datetime immediately.

3. **CHAIN TOOLS** - You can call multiple tools in sequence. After getting one result, you can call another tool.

4. **FINAL ANSWER** - Only after you have gathered all needed information using tools, provide a complete answer summarizing the results.

## Examples of correct behavior:

User: "What processes are running?"
→ IMMEDIATELY call list_processes tool, then summarize the results

User: "What's the weather?"
→ Call web_search with query "weather today", then summarize

User: "What time is it?"
→ Call get_datetime immediately

User: "Remember my name is John"
→ Call memory_save with key="user_name", value="John"

NEVER respond with just text asking if the user wants you to do something. USE THE TOOLS!

Respond in {language}."#
        )
    }

    /// Run the agent on a user query
    pub async fn run(&self, user_query: &str) -> Result<InteractionLog> {
        let interaction_id = uuid::Uuid::new_v4().to_string();
        let start_time = Utc::now();

        info!(
            interaction_id = %interaction_id,
            query = %user_query,
            "Starting agent run"
        );

        let mut messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_query),
        ];

        let tools = self.registry.to_json_schemas();
        let tools_ref = if tools.is_empty() {
            None
        } else {
            Some(tools.as_slice())
        };

        let mut steps: Vec<ReasoningStep> = Vec::new();
        let mut final_response: Option<String> = None;
        let max_steps = self.config.general.max_steps;

        for step_num in 1..=max_steps {
            debug!(step = step_num, "Executing step");

            // Get LLM response
            let response = self.provider.generate(&messages, tools_ref).await?;

            // Create step record
            let mut step = ReasoningStep {
                step_number: step_num,
                timestamp: Utc::now().to_rfc3339(),
                thought: response.content.clone(),
                tool_call: None,
                tool_result: None,
                is_final: false,
            };

            // Check if we have tool calls
            if response.has_tool_calls() {
                for tool_call in &response.tool_calls {
                    info!(
                        tool = %tool_call.name,
                        args = %tool_call.arguments,
                        "Executing tool"
                    );

                    step.tool_call = Some(ToolCallRecord {
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                    });

                    // Execute the tool
                    let result = self
                        .registry
                        .execute(&tool_call.name, tool_call.arguments.clone())
                        .await?;

                    let result_str = if result.success {
                        result.output
                    } else {
                        format!("Error: {}", result.error.unwrap_or_default())
                    };

                    step.tool_result = Some(result_str.clone());

                    info!(
                        tool = %tool_call.name,
                        success = result.success,
                        "Tool execution complete"
                    );

                    // Add assistant message with tool call info
                    if let Some(thought) = &response.content {
                        messages.push(Message::assistant(thought));
                    }

                    // Add tool result
                    messages.push(Message::tool(&result_str, &tool_call.id));
                }
            } else {
                // No tool calls - this is the final response
                step.is_final = true;
                final_response = response.content.clone();

                if let Some(ref callback) = self.step_callback {
                    callback(&step);
                }

                steps.push(step);
                break;
            }

            // Notify callback
            if let Some(ref callback) = self.step_callback {
                callback(&step);
            }

            steps.push(step);

            // Check if we should stop
            if response.is_complete() && !response.has_tool_calls() {
                final_response = response.content;
                break;
            }
        }

        // If we hit max steps without a final response
        if final_response.is_none() && steps.len() >= max_steps {
            warn!("Reached maximum steps without final response");
            final_response = Some(format!(
                "Достиг максимального количества шагов ({}). Вот что я узнал:\n{}",
                max_steps,
                steps
                    .iter()
                    .filter_map(|s| s.thought.clone())
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        let log = InteractionLog {
            id: interaction_id,
            timestamp: start_time.to_rfc3339(),
            user_query: user_query.to_string(),
            steps: steps.clone(),
            final_response: final_response.clone(),
            total_steps: steps.len(),
            success: final_response.is_some(),
        };

        info!(
            steps = log.total_steps,
            success = log.success,
            "Agent run complete"
        );

        Ok(log)
    }
}

/// Builder for creating agents
pub struct AgentBuilder {
    provider: Option<Arc<dyn LlmProvider>>,
    registry: ToolRegistry,
    config: Config,
}

impl AgentBuilder {
    pub fn new(config: Config) -> Self {
        Self {
            provider: None,
            registry: ToolRegistry::new(),
            config,
        }
    }

    pub fn with_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn with_registry(mut self, registry: ToolRegistry) -> Self {
        self.registry = registry;
        self
    }

    pub fn build(self) -> Result<Agent> {
        let provider = self
            .provider
            .ok_or_else(|| anyhow::anyhow!("LLM provider is required"))?;

        Ok(Agent::new(provider, Arc::new(self.registry), self.config))
    }
}
