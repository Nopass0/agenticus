use crate::tools::{Tool, ToolParameter, ToolResult, ToolSchema};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::process::Command;

/// Tool to type text into the active window
pub struct TypeTextTool;

#[async_trait]
impl Tool for TypeTextTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "type_text",
            "Type text into the currently active window. Works like keyboard input.",
        )
        .with_param(ToolParameter::string("text", "The text to type", true))
        .with_param(ToolParameter::number(
            "delay_ms",
            "Delay between keystrokes in milliseconds (default: 12)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Text is required"))?;

        let delay = params
            .get("delay_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(12);

        #[cfg(target_os = "linux")]
        {
            // Use xdotool on Linux
            let output = Command::new("xdotool")
                .args(["type", "--delay", &delay.to_string(), text])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Typed text: '{}'", text)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "xdotool failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!(
                    "Failed to run xdotool: {}. Install it with: sudo apt install xdotool",
                    e
                ))),
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Use PowerShell on Windows
            let escaped_text = text.replace("'", "''");
            let script = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}')"#,
                escaped_text.replace("{", "{{").replace("}", "}}").replace("+", "{{+}}").replace("^", "{{^}}").replace("%", "{{%}}")
            );

            let output = Command::new("powershell")
                .args(["-Command", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Typed text: '{}'", text)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "PowerShell failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to run PowerShell: {}", e))),
            }
        }

        #[cfg(target_os = "macos")]
        {
            // Use osascript on macOS
            let escaped_text = text.replace("\\", "\\\\").replace("\"", "\\\"");
            let script = format!(
                r#"tell application "System Events" to keystroke "{}""#,
                escaped_text
            );

            let output = Command::new("osascript")
                .args(["-e", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Typed text: '{}'", text)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "osascript failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to run osascript: {}", e))),
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Ok(ToolResult::error("Text input not supported on this platform"))
        }
    }
}

/// Tool to press special keys or key combinations
pub struct PressKeyTool;

#[async_trait]
impl Tool for PressKeyTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "press_key",
            "Press a key or key combination (e.g., 'Return', 'ctrl+s', 'alt+F4')",
        )
        .with_param(ToolParameter::string(
            "key",
            "Key to press: Return, Escape, Tab, BackSpace, Delete, ctrl+s, alt+F4, etc.",
            true,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Key is required"))?;

        #[cfg(target_os = "linux")]
        {
            let output = Command::new("xdotool")
                .args(["key", key])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Pressed key: {}", key)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "xdotool failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to run xdotool: {}", e))),
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Convert key notation to SendKeys format
            let sendkeys_key = match key.to_lowercase().as_str() {
                "return" | "enter" => "{ENTER}",
                "escape" | "esc" => "{ESC}",
                "tab" => "{TAB}",
                "backspace" => "{BACKSPACE}",
                "delete" => "{DELETE}",
                "up" => "{UP}",
                "down" => "{DOWN}",
                "left" => "{LEFT}",
                "right" => "{RIGHT}",
                "home" => "{HOME}",
                "end" => "{END}",
                "ctrl+s" => "^s",
                "ctrl+a" => "^a",
                "ctrl+c" => "^c",
                "ctrl+v" => "^v",
                "ctrl+z" => "^z",
                "alt+f4" => "%{F4}",
                _ => key,
            };

            let script = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}')"#,
                sendkeys_key
            );

            let output = Command::new("powershell")
                .args(["-Command", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Pressed key: {}", key)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "PowerShell failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to run PowerShell: {}", e))),
            }
        }

        #[cfg(target_os = "macos")]
        {
            let (modifiers, actual_key) = if key.contains('+') {
                let parts: Vec<&str> = key.split('+').collect();
                let mods = parts[..parts.len() - 1].join(" down, ") + " down";
                let key = parts.last().unwrap();
                (Some(mods), *key)
            } else {
                (None, key)
            };

            let key_code = match actual_key.to_lowercase().as_str() {
                "return" | "enter" => "return",
                "escape" | "esc" => "escape",
                "tab" => "tab",
                "delete" => "delete",
                _ => actual_key,
            };

            let script = if let Some(mods) = modifiers {
                format!(
                    r#"tell application "System Events" to key code {} using {{{}}}"#,
                    key_code, mods
                )
            } else {
                format!(
                    r#"tell application "System Events" to keystroke return"#
                )
            };

            let output = Command::new("osascript")
                .args(["-e", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Pressed key: {}", key)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "osascript failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to run osascript: {}", e))),
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Ok(ToolResult::error("Key press not supported on this platform"))
        }
    }
}

/// Tool to click mouse at position or current location
pub struct MouseClickTool;

#[async_trait]
impl Tool for MouseClickTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mouse_click",
            "Click the mouse at current position or specified coordinates",
        )
        .with_param(ToolParameter::number("x", "X coordinate (optional)", false))
        .with_param(ToolParameter::number("y", "Y coordinate (optional)", false))
        .with_param(
            ToolParameter::string("button", "Mouse button: left, right, middle (default: left)", false)
                .with_enum(vec!["left".to_string(), "right".to_string(), "middle".to_string()]),
        )
        .with_param(ToolParameter::number(
            "clicks",
            "Number of clicks (default: 1, use 2 for double-click)",
            false,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let x = params.get("x").and_then(|v| v.as_i64());
        let y = params.get("y").and_then(|v| v.as_i64());
        let button = params
            .get("button")
            .and_then(|v| v.as_str())
            .unwrap_or("left");
        let clicks = params.get("clicks").and_then(|v| v.as_u64()).unwrap_or(1);

        #[cfg(target_os = "linux")]
        {
            let button_num = match button {
                "left" => "1",
                "right" => "3",
                "middle" => "2",
                _ => "1",
            };

            // Move to position if specified
            if let (Some(x_val), Some(y_val)) = (x, y) {
                let move_output = Command::new("xdotool")
                    .args(["mousemove", &x_val.to_string(), &y_val.to_string()])
                    .output();

                if let Ok(o) = move_output {
                    if !o.status.success() {
                        return Ok(ToolResult::error("Failed to move mouse"));
                    }
                }
            }

            // Click
            for _ in 0..clicks {
                let click_output = Command::new("xdotool")
                    .args(["click", button_num])
                    .output();

                if let Ok(o) = click_output {
                    if !o.status.success() {
                        return Ok(ToolResult::error(format!(
                            "Click failed: {}",
                            String::from_utf8_lossy(&o.stderr)
                        )));
                    }
                }
            }

            let pos_str = if let (Some(x_val), Some(y_val)) = (x, y) {
                format!(" at ({}, {})", x_val, y_val)
            } else {
                String::new()
            };

            Ok(ToolResult::success(format!(
                "{} click{}{} performed",
                button,
                if clicks > 1 {
                    format!(" x{}", clicks)
                } else {
                    String::new()
                },
                pos_str
            )))
        }

        #[cfg(target_os = "windows")]
        {
            let mut script = String::new();
            script.push_str("Add-Type -AssemblyName System.Windows.Forms; ");

            if let (Some(x_val), Some(y_val)) = (x, y) {
                script.push_str(&format!(
                    "[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({}, {}); ",
                    x_val, y_val
                ));
            }

            // For clicking we need to use mouse_event via P/Invoke
            script.push_str(r#"
$signature = @'
[DllImport("user32.dll")]
public static extern void mouse_event(int dwFlags, int dx, int dy, int dwData, int dwExtraInfo);
'@
$mouse = Add-Type -MemberDefinition $signature -Name "MouseEvent" -Namespace "Win32" -PassThru
"#);

            let (down_flag, up_flag) = match button {
                "left" => ("0x0002", "0x0004"),
                "right" => ("0x0008", "0x0010"),
                "middle" => ("0x0020", "0x0040"),
                _ => ("0x0002", "0x0004"),
            };

            for _ in 0..clicks {
                script.push_str(&format!(
                    "$mouse::mouse_event({}, 0, 0, 0, 0); $mouse::mouse_event({}, 0, 0, 0, 0); ",
                    down_flag, up_flag
                ));
            }

            let output = Command::new("powershell")
                .args(["-Command", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Mouse {} click performed", button)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "Click failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed: {}", e))),
            }
        }

        #[cfg(target_os = "macos")]
        {
            Ok(ToolResult::error("Mouse click via osascript requires accessibility permissions. Use System Preferences > Security & Privacy > Accessibility"))
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Ok(ToolResult::error("Mouse click not supported on this platform"))
        }
    }
}

/// Tool to focus a window by name
pub struct FocusWindowTool;

#[async_trait]
impl Tool for FocusWindowTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "focus_window",
            "Focus/activate a window by its title or application name",
        )
        .with_param(ToolParameter::string(
            "name",
            "Window title or application name to search for",
            true,
        ))
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Name is required"))?;

        #[cfg(target_os = "linux")]
        {
            let output = Command::new("xdotool")
                .args(["search", "--name", name, "windowactivate"])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Focused window: {}", name)))
                }
                Ok(_) => Ok(ToolResult::error(format!(
                    "Window '{}' not found",
                    name
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed: {}", e))),
            }
        }

        #[cfg(target_os = "windows")]
        {
            let script = format!(
                r#"
$wshell = New-Object -ComObject wscript.shell
$windows = Get-Process | Where-Object {{$_.MainWindowTitle -like '*{}*'}} | Select-Object -First 1
if ($windows) {{
    $wshell.AppActivate($windows.MainWindowTitle)
    'Focused'
}} else {{
    'NotFound'
}}
"#,
                name
            );

            let output = Command::new("powershell")
                .args(["-Command", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    let result = String::from_utf8_lossy(&o.stdout);
                    if result.contains("Focused") {
                        Ok(ToolResult::success(format!("Focused window: {}", name)))
                    } else {
                        Ok(ToolResult::error(format!("Window '{}' not found", name)))
                    }
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "Failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed: {}", e))),
            }
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                r#"tell application "{}" to activate"#,
                name
            );

            let output = Command::new("osascript")
                .args(["-e", &script])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    Ok(ToolResult::success(format!("Focused: {}", name)))
                }
                Ok(o) => Ok(ToolResult::error(format!(
                    "Failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed: {}", e))),
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Ok(ToolResult::error("Window focus not supported on this platform"))
        }
    }
}

pub fn register_input_tools(registry: &mut crate::tools::ToolRegistry) {
    registry.add_tool(TypeTextTool);
    registry.add_tool(PressKeyTool);
    registry.add_tool(MouseClickTool);
    registry.add_tool(FocusWindowTool);
}
