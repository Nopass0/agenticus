use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Tool to open URLs in the default browser
pub struct OpenBrowserTool;

#[async_trait]
impl Tool for OpenBrowserTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("open_browser", "Open a URL in the default web browser")
            .with_param(ToolParameter::string("url", "The URL to open", true))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(
                "Invalid URL. Must start with http:// or https://",
            ));
        }

        match webbrowser::open(url) {
            Ok(_) => Ok(ToolResult::success(format!(
                "Successfully opened {} in default browser",
                url
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to open browser: {}", e))),
        }
    }
}

pub fn register_browser_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(OpenBrowserTool);
}
