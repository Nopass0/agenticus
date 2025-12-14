use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::Value;
use std::process::Command;
use tracing::{debug, warn};

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

/// Tool to fetch web page content with JavaScript rendering using headless browser
pub struct HeadlessBrowserTool;

impl HeadlessBrowserTool {
    /// Find available browser for headless mode
    fn find_browser() -> Option<(String, Vec<&'static str>)> {
        // List of browsers to try with their headless arguments
        let browsers = vec![
            // Chrome/Chromium
            ("google-chrome", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("google-chrome-stable", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("chromium", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("chromium-browser", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            // Edge
            ("microsoft-edge", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("msedge", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
        ];

        #[cfg(target_os = "windows")]
        let browsers = vec![
            // Windows paths
            ("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            // Try PATH
            ("chrome", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
            ("msedge", vec!["--headless=new", "--disable-gpu", "--no-sandbox", "--dump-dom"]),
        ];

        for (browser, args) in browsers {
            // Check if browser exists
            #[cfg(target_os = "windows")]
            {
                if std::path::Path::new(browser).exists() {
                    return Some((browser.to_string(), args));
                }
                if which::which(browser).is_ok() {
                    return Some((browser.to_string(), args));
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                if which::which(browser).is_ok() {
                    return Some((browser.to_string(), args));
                }
            }
        }

        None
    }

    /// Fetch page with JavaScript rendering
    fn fetch_with_js(url: &str, _timeout_secs: u64) -> Result<String> {
        let (browser, mut args) = Self::find_browser()
            .ok_or_else(|| anyhow::anyhow!("No suitable browser found for headless mode. Install Chrome or Edge."))?;

        debug!("Using browser: {} for headless fetch", browser);

        // Add URL to args
        args.push(url);

        let output = Command::new(&browser)
            .args(&args[..args.len()-1]) // All args except URL
            .arg(url) // URL as last argument
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Browser failed: {}", stderr));
        }

        let html = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(html)
    }
}

#[async_trait]
impl Tool for HeadlessBrowserTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "fetch_with_js",
            "Fetch a web page with JavaScript rendering using a headless browser. Use this for pages that require JavaScript to load content.",
        )
        .with_param(ToolParameter::string("url", "The URL to fetch", true))
        .with_param(ToolParameter::string(
            "selector",
            "CSS selector to extract specific content (optional)",
            false,
        ))
        .with_param(ToolParameter::number(
            "max_length",
            "Maximum length of extracted text (default: 10000)",
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
            .unwrap_or(10000) as usize;

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(
                "Invalid URL. Must start with http:// or https://",
            ));
        }

        debug!("Fetching with JS: {}", url);

        // Fetch with headless browser
        let html = match Self::fetch_with_js(url, 30) {
            Ok(h) => h,
            Err(e) => {
                warn!("Headless browser failed: {}", e);
                return Ok(ToolResult::error(format!(
                    "Failed to fetch with headless browser: {}. Make sure Chrome or Edge is installed.",
                    e
                )));
            }
        };

        debug!("Got HTML length: {}", html.len());

        // Parse HTML
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
            // Extract main text content from body
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
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");

        // Truncate if needed (safely handle UTF-8)
        let text = if text.chars().count() > max_length {
            let truncated: String = text.chars().take(max_length).collect();
            format!("{}... [truncated]", truncated)
        } else {
            text
        };

        Ok(ToolResult::success_with_data(
            text.clone(),
            serde_json::json!({
                "url": url,
                "content_length": text.len(),
                "js_rendered": true
            }),
        ))
    }
}

/// Tool to extract links from a JS-rendered page
pub struct ExtractLinksJsTool;

#[async_trait]
impl Tool for ExtractLinksJsTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "extract_links_js",
            "Extract links from a JavaScript-rendered page using headless browser.",
        )
        .with_param(ToolParameter::string("url", "The URL to fetch", true))
        .with_param(ToolParameter::string(
            "selector",
            "CSS selector for link elements (default: 'a')",
            false,
        ))
        .with_param(ToolParameter::number(
            "max_links",
            "Maximum number of links to return (default: 20)",
            false,
        ))
        .with_param(ToolParameter::string(
            "filter",
            "Only return links containing this substring in URL or text",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("URL is required"))?;

        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .unwrap_or("a");

        let max_links = params
            .get("max_links")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let filter = params.get("filter").and_then(|v| v.as_str());

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(
                "Invalid URL. Must start with http:// or https://",
            ));
        }

        debug!("Extracting links from: {}", url);

        // Fetch with headless browser
        let html = match HeadlessBrowserTool::fetch_with_js(url, 30) {
            Ok(h) => h,
            Err(e) => {
                warn!("Headless browser failed: {}", e);
                return Ok(ToolResult::error(format!(
                    "Failed to fetch with headless browser: {}",
                    e
                )));
            }
        };

        // Parse HTML
        let document = Html::parse_document(&html);
        let link_selector = match Selector::parse(selector) {
            Ok(s) => s,
            Err(_) => return Ok(ToolResult::error("Invalid CSS selector")),
        };

        let base_url = url::Url::parse(url).ok();
        let mut links = Vec::new();

        for element in document.select(&link_selector) {
            if let Some(href) = element.value().attr("href") {
                let text = element.text().collect::<Vec<_>>().join(" ").trim().to_string();

                // Resolve relative URLs
                let full_url = if href.starts_with("http") {
                    href.to_string()
                } else if let Some(base) = &base_url {
                    base.join(href).map(|u| u.to_string()).unwrap_or_default()
                } else {
                    continue;
                };

                if full_url.is_empty() {
                    continue;
                }

                // Apply filter
                if let Some(f) = filter {
                    let f_lower = f.to_lowercase();
                    if !full_url.to_lowercase().contains(&f_lower)
                        && !text.to_lowercase().contains(&f_lower)
                    {
                        continue;
                    }
                }

                // Avoid duplicates
                if !links.iter().any(|l: &serde_json::Value| {
                    l.get("url").and_then(|u| u.as_str()) == Some(&full_url)
                }) {
                    links.push(serde_json::json!({
                        "url": full_url,
                        "text": if text.is_empty() { href.to_string() } else { text }
                    }));
                }

                if links.len() >= max_links {
                    break;
                }
            }
        }

        // Format output
        let mut output = format!("Found {} links:\n\n", links.len());
        for (i, link) in links.iter().enumerate() {
            output.push_str(&format!(
                "{}. {}\n   {}\n\n",
                i + 1,
                link.get("text").and_then(|t| t.as_str()).unwrap_or(""),
                link.get("url").and_then(|u| u.as_str()).unwrap_or("")
            ));
        }

        Ok(ToolResult::success_with_data(
            output,
            serde_json::json!({
                "url": url,
                "links": links,
                "count": links.len()
            }),
        ))
    }
}

pub fn register_browser_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(OpenBrowserTool);
    registry.add_tool(HeadlessBrowserTool);
    registry.add_tool(ExtractLinksJsTool);
}
