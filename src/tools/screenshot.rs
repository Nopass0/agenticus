use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::fs;

/// Tool to take screenshots
pub struct ScreenshotTool;

#[async_trait]
impl Tool for ScreenshotTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "take_screenshot",
            "Capture a screenshot of the entire screen or a specific monitor. Returns base64 encoded image.",
        )
        .with_param(ToolParameter::number(
            "monitor",
            "Monitor index (0 for primary, default: 0)",
            false,
        ))
        .with_param(ToolParameter::string(
            "save_path",
            "Optional path to save the screenshot file",
            false,
        ))
        .with_param(ToolParameter::boolean(
            "return_base64",
            "Return base64 encoded image data (default: true)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let monitor_idx = params
            .get("monitor")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let save_path = params.get("save_path").and_then(|v| v.as_str());
        let return_base64 = params
            .get("return_base64")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Get screens
        let screens = screenshots::Screen::all().map_err(|e| anyhow::anyhow!("Failed to get screens: {}", e))?;

        if screens.is_empty() {
            return Ok(ToolResult::error("No screens available for capture"));
        }

        let screen = screens.get(monitor_idx).unwrap_or(&screens[0]);

        // Capture screenshot
        let image = screen.capture().map_err(|e| anyhow::anyhow!("Failed to capture screenshot: {}", e))?;

        let width = image.width();
        let height = image.height();

        // Convert to PNG bytes
        let mut png_bytes = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
            encoder.write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )?;
        }

        // Save to file if requested
        let saved_path = if let Some(path) = save_path {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &png_bytes)?;
            Some(path.to_string_lossy().to_string())
        } else {
            None
        };

        // Create result
        let mut result = serde_json::json!({
            "width": width,
            "height": height,
            "monitor_index": monitor_idx,
            "size_bytes": png_bytes.len()
        });

        if let Some(path) = &saved_path {
            result["saved_path"] = Value::String(path.clone());
        }

        let output = if let Some(path) = &saved_path {
            format!(
                "Screenshot captured ({}x{}) and saved to: {}",
                width, height, path
            )
        } else {
            format!("Screenshot captured ({}x{})", width, height)
        };

        if return_base64 {
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
            result["base64"] = Value::String(b64);
        }

        Ok(ToolResult::success_with_data(output, result))
    }
}

/// Tool to analyze a screenshot using AI (returns description of what's visible)
pub struct AnalyzeScreenshotTool;

#[async_trait]
impl Tool for AnalyzeScreenshotTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "analyze_screenshot",
            "Take a screenshot and analyze it with AI vision. Returns description of what's on screen, UI elements, text, etc.",
        )
        .with_param(ToolParameter::string(
            "question",
            "What to look for or question about the screenshot (e.g., 'find the OK button', 'read the error message')",
            false,
        ))
        .with_param(ToolParameter::number(
            "monitor",
            "Monitor index (0 for primary, default: 0)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let question = params
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe what you see on this screenshot. List any UI elements, buttons, text, windows, and their approximate positions.");

        let monitor_idx = params
            .get("monitor")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        // Get screens
        let screens = screenshots::Screen::all()
            .map_err(|e| anyhow::anyhow!("Failed to get screens: {}", e))?;

        if screens.is_empty() {
            return Ok(ToolResult::error("No screens available for capture"));
        }

        let screen = screens.get(monitor_idx).unwrap_or(&screens[0]);

        // Capture screenshot
        let image = screen.capture()
            .map_err(|e| anyhow::anyhow!("Failed to capture screenshot: {}", e))?;

        let width = image.width();
        let height = image.height();

        // Convert to PNG bytes
        let mut png_bytes = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
            encoder.write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )?;
        }

        // Encode to base64
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);

        // Create response with the image data and question
        // The actual AI analysis would be done by the main agent using the vision model
        // For now, we return the image data and question for the agent to process
        Ok(ToolResult::success_with_data(
            format!(
                "Screenshot captured ({}x{}). Question: {}\n\n[IMAGE_DATA_FOR_VISION_ANALYSIS]\nThe main agent should analyze this image to answer the question.",
                width, height, question
            ),
            serde_json::json!({
                "width": width,
                "height": height,
                "base64": b64,
                "question": question,
                "needs_vision_analysis": true
            }),
        ))
    }
}

/// Tool to find UI element by description
pub struct FindUIElementTool;

#[async_trait]
impl Tool for FindUIElementTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "find_ui_element",
            "Take a screenshot and find a UI element by description. Returns approximate coordinates.",
        )
        .with_param(ToolParameter::string(
            "element",
            "Description of the UI element to find (e.g., 'OK button', 'search field', 'close button')",
            true,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let element = params
            .get("element")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Element description is required"))?;

        // Get screens
        let screens = screenshots::Screen::all()
            .map_err(|e| anyhow::anyhow!("Failed to get screens: {}", e))?;

        if screens.is_empty() {
            return Ok(ToolResult::error("No screens available for capture"));
        }

        let screen = &screens[0];

        // Capture screenshot
        let image = screen.capture()
            .map_err(|e| anyhow::anyhow!("Failed to capture screenshot: {}", e))?;

        let width = image.width();
        let height = image.height();

        // Convert to PNG bytes
        let mut png_bytes = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
            encoder.write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )?;
        }

        // Encode to base64
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);

        Ok(ToolResult::success_with_data(
            format!(
                "Screenshot captured ({}x{}). Looking for: '{}'\n\n[NEEDS_VISION_ANALYSIS]\nAnalyze the image to find the element and return its approximate center coordinates (x, y).",
                width, height, element
            ),
            serde_json::json!({
                "width": width,
                "height": height,
                "base64": b64,
                "element_to_find": element,
                "needs_vision_analysis": true
            }),
        ))
    }
}

pub fn register_screenshot_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(ScreenshotTool);
    registry.add_tool(AnalyzeScreenshotTool);
    registry.add_tool(FindUIElementTool);
}
