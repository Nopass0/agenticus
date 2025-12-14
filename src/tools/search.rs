use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::Value;

/// Tool to fetch and extract content from web pages
pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "web_fetch",
            "Fetch a web page and extract its text content. Useful for reading articles, documentation, etc.",
        )
        .with_param(ToolParameter::string("url", "The URL to fetch", true))
        .with_param(ToolParameter::string(
            "selector",
            "CSS selector to extract specific content (optional, e.g., 'article', '.content', '#main')",
            false,
        ))
        .with_param(ToolParameter::number(
            "max_length",
            "Maximum length of extracted text (default: 5000)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let selector = params.get("selector").and_then(|v| v.as_str());
        let max_length = params
            .get("max_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(5000) as usize;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let response = client.get(url).send().await?;
        let status = response.status();

        if !status.is_success() {
            return Ok(ToolResult::error(format!(
                "HTTP error: {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        let html = response.text().await?;
        let document = Html::parse_document(&html);

        let text = if let Some(sel_str) = selector {
            match Selector::parse(sel_str) {
                Ok(sel) => {
                    let mut text = String::new();
                    for element in document.select(&sel) {
                        text.push_str(&element.text().collect::<Vec<_>>().join(" "));
                        text.push('\n');
                    }
                    text
                }
                Err(_) => return Ok(ToolResult::error("Invalid CSS selector")),
            }
        } else {
            // Extract main text content
            let body_selector = Selector::parse("body").unwrap();

            let mut text = String::new();
            if let Some(body) = document.select(&body_selector).next() {
                for node in body.text() {
                    let trimmed = node.trim();
                    if !trimmed.is_empty() {
                        text.push_str(trimmed);
                        text.push(' ');
                    }
                }
            }
            text
        };

        // Clean up whitespace
        let text = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        // Truncate if needed
        let text = if text.len() > max_length {
            format!("{}... [truncated]", &text[..max_length])
        } else {
            text
        };

        Ok(ToolResult::success_with_data(
            text.clone(),
            serde_json::json!({
                "url": url,
                "content_length": text.len(),
                "extracted_text": text
            }),
        ))
    }
}

/// Tool for web search using DuckDuckGo HTML
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "web_search",
            "Search the web using DuckDuckGo and return results. Works without API key.",
        )
        .with_param(ToolParameter::string("query", "The search query", true))
        .with_param(ToolParameter::number(
            "max_results",
            "Maximum number of results to return (default: 5)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Query is required"))?;

        let max_results = params
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        // Use DuckDuckGo HTML search
        let encoded_query = urlencoding::encode(query);
        let search_url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

        let response = client.get(&search_url).send().await?;
        let html = response.text().await?;
        let document = Html::parse_document(&html);

        let result_selector = Selector::parse(".result").unwrap();
        let title_selector = Selector::parse(".result__title a").unwrap();
        let snippet_selector = Selector::parse(".result__snippet").unwrap();
        let url_selector = Selector::parse(".result__url").unwrap();

        let mut results = Vec::new();
        let mut output = String::new();

        for (i, result) in document.select(&result_selector).take(max_results).enumerate() {
            let title = result
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<Vec<_>>().join(""))
                .unwrap_or_default();

            let snippet = result
                .select(&snippet_selector)
                .next()
                .map(|e| e.text().collect::<Vec<_>>().join(""))
                .unwrap_or_default();

            let url = result
                .select(&url_selector)
                .next()
                .map(|e| e.text().collect::<Vec<_>>().join("").trim().to_string())
                .unwrap_or_default();

            if !title.is_empty() {
                results.push(serde_json::json!({
                    "title": title.trim(),
                    "snippet": snippet.trim(),
                    "url": url.trim()
                }));

                output.push_str(&format!(
                    "{}. {}\n   {}\n   {}\n\n",
                    i + 1,
                    title.trim(),
                    snippet.trim(),
                    url.trim()
                ));
            }
        }

        if results.is_empty() {
            Ok(ToolResult::success("No search results found"))
        } else {
            Ok(ToolResult::success_with_data(
                output,
                serde_json::json!({
                    "query": query,
                    "results": results
                }),
            ))
        }
    }
}

pub fn register_search_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(WebFetchTool);
    registry.add_tool(WebSearchTool);
}

// Need urlencoding
fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
