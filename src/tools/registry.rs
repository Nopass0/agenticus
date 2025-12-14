use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            data: None,
            error: None,
        }
    }

    pub fn success_with_data(output: impl Into<String>, data: Value) -> Self {
        Self {
            success: true,
            output: output.into(),
            data: Some(data),
            error: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            data: None,
            error: Some(error.into()),
        }
    }
}

/// Schema for tool parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
}

impl ToolParameter {
    pub fn new(name: &str, description: &str, param_type: &str, required: bool) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            param_type: param_type.to_string(),
            required,
            enum_values: None,
        }
    }

    pub fn with_enum(mut self, values: Vec<String>) -> Self {
        self.enum_values = Some(values);
        self
    }

    pub fn string(name: &str, description: &str, required: bool) -> Self {
        Self::new(name, description, "string", required)
    }

    pub fn number(name: &str, description: &str, required: bool) -> Self {
        Self::new(name, description, "number", required)
    }

    pub fn boolean(name: &str, description: &str, required: bool) -> Self {
        Self::new(name, description, "boolean", required)
    }

    pub fn array(name: &str, description: &str, required: bool) -> Self {
        Self::new(name, description, "array", required)
    }

    pub fn object(name: &str, description: &str, required: bool) -> Self {
        Self::new(name, description, "object", required)
    }
}

/// Schema for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParameter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
}

impl ToolSchema {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            parameters: Vec::new(),
            returns: None,
        }
    }

    pub fn with_param(mut self, param: ToolParameter) -> Self {
        self.parameters.push(param);
        self
    }

    pub fn with_returns(mut self, returns: &str) -> Self {
        self.returns = Some(returns.to_string());
        self
    }

    /// Convert to JSON schema format for LLM
    pub fn to_json_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            let mut prop = serde_json::Map::new();
            prop.insert("type".to_string(), Value::String(param.param_type.clone()));
            prop.insert("description".to_string(), Value::String(param.description.clone()));

            if let Some(enum_vals) = &param.enum_values {
                prop.insert(
                    "enum".to_string(),
                    Value::Array(enum_vals.iter().map(|v| Value::String(v.clone())).collect()),
                );
            }

            properties.insert(param.name.clone(), Value::Object(prop));

            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }

        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                    "required": required
                }
            }
        })
    }
}

/// Trait for tools
#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, params: Value) -> Result<ToolResult>;
}

/// Type for boxed async tool functions
pub type BoxedToolFn = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send>> + Send + Sync,
>;

/// A simple function-based tool
pub struct FnTool {
    schema: ToolSchema,
    handler: BoxedToolFn,
}

impl FnTool {
    pub fn new<F, Fut>(schema: ToolSchema, handler: F) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolResult>> + Send + 'static,
    {
        let handler: BoxedToolFn = Arc::new(move |params| Box::pin(handler(params)));
        Self { schema, handler }
    }
}

#[async_trait]
impl Tool for FnTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, params: Value) -> Result<ToolResult> {
        (self.handler)(params).await
    }
}

/// Registry for all tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Add a tool implementing the Tool trait
    pub fn add_tool<T: Tool + 'static>(&mut self, tool: T) -> &mut Self {
        let schema = tool.schema();
        self.tools.insert(schema.name.clone(), Arc::new(tool));
        self
    }

    /// Add a simple function-based tool
    pub fn add_instrument<F, Fut>(
        &mut self,
        name: &str,
        description: &str,
        parameters: Vec<ToolParameter>,
        handler: F,
    ) -> &mut Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolResult>> + Send + 'static,
    {
        let mut schema = ToolSchema::new(name, description);
        for param in parameters {
            schema = schema.with_param(param);
        }

        let tool = FnTool::new(schema, handler);
        self.add_tool(tool);
        self
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Get all tool schemas
    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    /// Get all tool schemas as JSON for LLM
    pub fn to_json_schemas(&self) -> Vec<Value> {
        self.schemas().iter().map(|s| s.to_json_schema()).collect()
    }

    /// Get tool names
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, params: Value) -> Result<ToolResult> {
        match self.get(name) {
            Some(tool) => tool.execute(params).await,
            None => Ok(ToolResult::error(format!("Tool '{}' not found", name))),
        }
    }

    /// Get formatted tool list for LLM prompt
    pub fn format_for_prompt(&self) -> String {
        let mut result = String::from("Available tools:\n\n");

        for schema in self.schemas() {
            result.push_str(&format!("## {}\n", schema.name));
            result.push_str(&format!("{}\n", schema.description));

            if !schema.parameters.is_empty() {
                result.push_str("Parameters:\n");
                for param in &schema.parameters {
                    let req = if param.required { " (required)" } else { " (optional)" };
                    result.push_str(&format!(
                        "  - {}: {} [{}]{}\n",
                        param.name, param.description, param.param_type, req
                    ));
                }
            }
            result.push('\n');
        }

        result
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
