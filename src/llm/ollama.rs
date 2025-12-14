use super::{LlmProvider, LlmResponse, Message, Role, ToolCall};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Ollama API provider
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaToolCall {
    function: OllamaFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
}

impl From<&Message> for OllamaMessage {
    fn from(msg: &Message) -> Self {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        Self {
            role: role.to_string(),
            content: msg.content.clone(),
            tool_calls: None,
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn generate(
        &self,
        messages: &[Message],
        tools: Option<&[Value]>,
    ) -> Result<LlmResponse> {
        let ollama_messages: Vec<OllamaMessage> = messages.iter().map(|m| m.into()).collect();

        let request = OllamaRequest {
            model: self.model.clone(),
            messages: ollama_messages,
            stream: false,
            tools: tools.map(|t| t.to_vec()),
            options: OllamaOptions { temperature: 0.7 },
        };

        let url = format!("{}/api/chat", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Ollama API error ({}): {}", status, error_text);
        }

        let ollama_response: OllamaResponse = response.json().await?;

        let tool_calls = ollama_response
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, tc)| ToolCall {
                id: format!("call_{}", i),
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        Ok(LlmResponse {
            content: if ollama_response.message.content.is_empty() {
                None
            } else {
                Some(ollama_response.message.content)
            },
            tool_calls,
            finish_reason: ollama_response.done_reason,
        })
    }

    fn name(&self) -> &str {
        "Ollama"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
