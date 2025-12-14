use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// Tool to make HTTP API requests
pub struct HttpRequestTool;

#[async_trait]
impl Tool for HttpRequestTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "http_request",
            "Make HTTP requests to APIs. Supports GET, POST, PUT, DELETE methods with headers and body.",
        )
        .with_param(ToolParameter::string("url", "The URL to request", true))
        .with_param(
            ToolParameter::string("method", "HTTP method: GET, POST, PUT, DELETE", false)
                .with_enum(vec![
                    "GET".to_string(),
                    "POST".to_string(),
                    "PUT".to_string(),
                    "DELETE".to_string(),
                    "PATCH".to_string(),
                ]),
        )
        .with_param(ToolParameter::object(
            "headers",
            "HTTP headers as key-value pairs (optional)",
            false,
        ))
        .with_param(ToolParameter::string(
            "body",
            "Request body for POST/PUT (optional, JSON string)",
            false,
        ))
        .with_param(ToolParameter::number(
            "timeout",
            "Request timeout in seconds (default: 30)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let method = params
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();

        let timeout = params
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout))
            .build()?;

        let mut request = match method.as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => return Ok(ToolResult::error(format!("Unsupported method: {}", method))),
        };

        // Add headers
        if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(val_str) = value.as_str() {
                    request = request.header(key.as_str(), val_str);
                }
            }
        }

        // Add body
        if let Some(body) = params.get("body") {
            if let Some(body_str) = body.as_str() {
                request = request.header("Content-Type", "application/json").body(body_str.to_string());
            } else {
                request = request.json(body);
            }
        }

        let response = request.send().await?;
        let status = response.status();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response.text().await?;

        // Try to parse as JSON for better formatting
        let body_json: Value = serde_json::from_str(&body).unwrap_or(Value::String(body.clone()));

        let result = serde_json::json!({
            "status_code": status.as_u16(),
            "status_text": status.canonical_reason().unwrap_or("Unknown"),
            "headers": headers,
            "body": body_json,
            "success": status.is_success()
        });

        let output = if status.is_success() {
            let body_preview = if body.chars().count() > 1000 {
                format!("{}... [truncated]", body.chars().take(1000).collect::<String>())
            } else {
                body
            };
            format!("HTTP {} {}\n\n{}", status.as_u16(), status.canonical_reason().unwrap_or(""), body_preview)
        } else {
            format!("HTTP Error {} {}\n\n{}", status.as_u16(), status.canonical_reason().unwrap_or(""), body)
        };

        Ok(ToolResult::success_with_data(output, result))
    }
}

pub fn register_api_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(HttpRequestTool);
}
