use super::{LlmProvider, LlmResponse, Message, Role, ToolCall};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// OpenRouter API provider
pub struct OpenRouterProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl OpenRouterProvider {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<OpenRouterMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenRouterMessage {
    role: String,
    content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenRouterToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ContentPart {
    #[serde(rename = "type")]
    part_type: String,
    text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenRouterToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenRouterFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenRouterFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
    #[serde(default)]
    error: Option<OpenRouterError>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
    finish_reason: Option<String>,
}

fn convert_message(msg: &Message) -> OpenRouterMessage {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    OpenRouterMessage {
        role: role.to_string(),
        content: MessageContent::Text(msg.content.clone()),
        tool_calls: None,
        tool_call_id: msg.tool_call_id.clone(),
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn generate(
        &self,
        messages: &[Message],
        tools: Option<&[Value]>,
    ) -> Result<LlmResponse> {
        let openrouter_messages: Vec<OpenRouterMessage> =
            messages.iter().map(convert_message).collect();

        let request = OpenRouterRequest {
            model: self.model.clone(),
            messages: openrouter_messages,
            tools: tools.map(|t| t.to_vec()),
            tool_choice: tools.map(|_| "auto".to_string()),
            temperature: 0.7,
            max_tokens: 4096,
        };

        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/agenticus")
            .header("X-Title", "Agenticus AI Agent")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("OpenRouter API error ({}): {}", status, error_text);
        }

        let openrouter_response: OpenRouterResponse = response.json().await?;

        if let Some(error) = openrouter_response.error {
            anyhow::bail!("OpenRouter error: {}", error.message);
        }

        let choice = openrouter_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))?;

        let content = match &choice.message.content {
            MessageContent::Text(s) if !s.is_empty() => Some(s.clone()),
            MessageContent::Parts(parts) => {
                let text: String = parts
                    .iter()
                    .filter_map(|p| p.text.clone())
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        };

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let arguments: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments,
                }
            })
            .collect();

        Ok(LlmResponse {
            content,
            tool_calls,
            finish_reason: choice.finish_reason,
        })
    }

    fn name(&self) -> &str {
        "OpenRouter"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
