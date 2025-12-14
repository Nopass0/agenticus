use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::Value;
use tracing::{debug, warn};

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

        // Truncate if needed (safely handle UTF-8 boundaries)
        let text = if text.len() > max_length {
            // Find the last valid character boundary at or before max_length
            let truncate_at = text
                .char_indices()
                .take_while(|(i, _)| *i < max_length)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            format!("{}... [truncated]", &text[..truncate_at])
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

/// Tool for web search using multiple engines
pub struct WebSearchTool;

impl WebSearchTool {
    async fn search_duckduckgo(client: &reqwest::Client, query: &str, max_results: usize) -> Result<Vec<serde_json::Value>> {
        let encoded_query = urlencoding::encode(query);
        // Use lite version which is more scraper-friendly
        let search_url = format!("https://lite.duckduckgo.com/lite/?q={}", encoded_query);

        debug!("DuckDuckGo Lite search URL: {}", search_url);

        let response = client
            .get(&search_url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.9,ru;q=0.8")
            .header("Accept-Encoding", "gzip, deflate")
            .header("DNT", "1")
            .header("Connection", "keep-alive")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await?;

        let status = response.status();
        debug!("DuckDuckGo response status: {}", status);

        if !status.is_success() {
            return Err(anyhow::anyhow!("DuckDuckGo returned status: {}", status));
        }

        let html = response.text().await?;
        debug!("DuckDuckGo HTML length: {}", html.len());

        let document = Html::parse_document(&html);

        // DDG Lite uses table-based layout with specific structure
        let mut results = Vec::new();

        // In DDG Lite, results are in table rows with class "result-link" for links
        // and following rows for snippets
        if let Ok(link_selector) = Selector::parse("a.result-link") {
            for link_elem in document.select(&link_selector).take(max_results) {
                let title = link_elem.text().collect::<Vec<_>>().join("").trim().to_string();
                let url = link_elem.value().attr("href").unwrap_or("").to_string();

                if !title.is_empty() && !url.is_empty() {
                    // Extract actual URL from DDG redirect
                    let actual_url = if url.contains("uddg=") {
                        url.split("uddg=").nth(1)
                            .and_then(|u| urlencoding::decode(u))
                            .unwrap_or(url.clone())
                    } else {
                        url.clone()
                    };

                    results.push(serde_json::json!({
                        "title": title,
                        "snippet": "",
                        "url": actual_url
                    }));
                }
            }
        }

        // If no results from lite selector, try standard HTML selectors
        if results.is_empty() {
            let result_selectors = vec![".result", ".web-result", ".results_links", "tr"];

            for selector_str in result_selectors {
                if let Ok(result_selector) = Selector::parse(selector_str) {
                    for result in document.select(&result_selector).take(max_results * 2) {
                        if let Ok(link_sel) = Selector::parse("a") {
                            if let Some(link) = result.select(&link_sel).next() {
                                let title = link.text().collect::<Vec<_>>().join("").trim().to_string();
                                if let Some(href) = link.value().attr("href") {
                                    if !title.is_empty() && (href.starts_with("http") || href.contains("uddg=")) {
                                        let url = if href.contains("uddg=") {
                                            href.split("uddg=").nth(1)
                                                .and_then(|u| urlencoding::decode(u))
                                                .unwrap_or_else(|| href.to_string())
                                        } else {
                                            href.to_string()
                                        };

                                        // Avoid duplicate titles
                                        if !results.iter().any(|r| r.get("title").and_then(|t| t.as_str()) == Some(&title)) {
                                            results.push(serde_json::json!({
                                                "title": title,
                                                "snippet": "",
                                                "url": url
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if results.len() >= max_results {
                        break;
                    }
                }
            }
        }

        results.truncate(max_results);
        Ok(results)
    }

    async fn search_bing(client: &reqwest::Client, query: &str, max_results: usize) -> Result<Vec<serde_json::Value>> {
        let encoded_query = urlencoding::encode(query);
        let search_url = format!("https://www.bing.com/search?q={}&count={}", encoded_query, max_results);

        debug!("Bing search URL: {}", search_url);

        let response = client
            .get(&search_url)
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Bing returned status: {}", response.status()));
        }

        let html = response.text().await?;
        let document = Html::parse_document(&html);

        debug!("Bing HTML length: {}", html.len());

        let mut results = Vec::new();

        // Bing results are in li.b_algo elements
        if let Ok(result_selector) = Selector::parse("li.b_algo") {
            for result in document.select(&result_selector).take(max_results) {
                let mut title = String::new();
                let mut url = String::new();
                let mut snippet = String::new();

                // Title is in h2 > a
                if let Ok(title_sel) = Selector::parse("h2 a") {
                    if let Some(elem) = result.select(&title_sel).next() {
                        title = elem.text().collect::<Vec<_>>().join("").trim().to_string();
                        if let Some(href) = elem.value().attr("href") {
                            url = href.to_string();
                        }
                    }
                }

                // Snippet is in .b_caption p
                if let Ok(snippet_sel) = Selector::parse(".b_caption p, p") {
                    if let Some(elem) = result.select(&snippet_sel).next() {
                        snippet = elem.text().collect::<Vec<_>>().join("").trim().to_string();
                    }
                }

                if !title.is_empty() && url.starts_with("http") {
                    results.push(serde_json::json!({
                        "title": title,
                        "snippet": snippet,
                        "url": url
                    }));
                }
            }
        }

        Ok(results)
    }

    async fn search_google_scrape(client: &reqwest::Client, query: &str, max_results: usize) -> Result<Vec<serde_json::Value>> {
        let encoded_query = urlencoding::encode(query);
        let search_url = format!("https://www.google.com/search?q={}&num={}", encoded_query, max_results);

        debug!("Google search URL: {}", search_url);

        let response = client
            .get(&search_url)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Google returned status: {}", response.status()));
        }

        let html = response.text().await?;
        let document = Html::parse_document(&html);

        let mut results = Vec::new();

        // Try to parse Google results
        if let Ok(result_selector) = Selector::parse("div.g, div[data-sokoban-container]") {
            for result in document.select(&result_selector).take(max_results) {
                let title = if let Ok(sel) = Selector::parse("h3") {
                    result.select(&sel).next()
                        .map(|e| e.text().collect::<Vec<_>>().join(""))
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let url = if let Ok(sel) = Selector::parse("a") {
                    result.select(&sel).next()
                        .and_then(|e| e.value().attr("href"))
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let snippet = if let Ok(sel) = Selector::parse("div.VwiC3b, span.aCOpRe") {
                    result.select(&sel).next()
                        .map(|e| e.text().collect::<Vec<_>>().join(""))
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                if !title.is_empty() && url.starts_with("http") {
                    results.push(serde_json::json!({
                        "title": title.trim(),
                        "snippet": snippet.trim(),
                        "url": url
                    }));
                }
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "web_search",
            "Search the web and return results. Tries multiple search engines.",
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
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        // Try DuckDuckGo Lite first (most reliable for scraping)
        let mut results = match Self::search_duckduckgo(&client, query, max_results).await {
            Ok(r) => {
                debug!("DuckDuckGo returned {} results", r.len());
                r
            }
            Err(e) => {
                warn!("DuckDuckGo search failed: {}", e);
                Vec::new()
            }
        };

        // If no results, try Bing
        if results.is_empty() {
            results = match Self::search_bing(&client, query, max_results).await {
                Ok(r) => {
                    debug!("Bing returned {} results", r.len());
                    r
                }
                Err(e) => {
                    warn!("Bing search failed: {}", e);
                    Vec::new()
                }
            };
        }

        // If still no results, try Google
        if results.is_empty() {
            results = match Self::search_google_scrape(&client, query, max_results).await {
                Ok(r) => {
                    debug!("Google returned {} results", r.len());
                    r
                }
                Err(e) => {
                    warn!("Google search failed: {}", e);
                    Vec::new()
                }
            };
        }

        if results.is_empty() {
            Ok(ToolResult::success_with_data(
                "No search results found. Search engines may be blocking requests or the query returned no matches.",
                serde_json::json!({
                    "query": query,
                    "results": [],
                    "error": "No results from any search engine"
                }),
            ))
        } else {
            let mut output = String::new();
            for (i, result) in results.iter().enumerate() {
                output.push_str(&format!(
                    "{}. {}\n   {}\n   {}\n\n",
                    i + 1,
                    result.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    result.get("snippet").and_then(|v| v.as_str()).unwrap_or(""),
                    result.get("url").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }

            Ok(ToolResult::success_with_data(
                output,
                serde_json::json!({
                    "query": query,
                    "results": results,
                    "count": results.len()
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

    pub fn decode(s: &str) -> Option<String> {
        url::form_urlencoded::parse(s.as_bytes())
            .next()
            .map(|(k, _)| k.into_owned())
    }
}
