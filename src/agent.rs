use crate::config::Config;
use crate::llm::{LlmProvider, Message};
use crate::tools::ToolRegistry;
use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Status of a task in the plan
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Parallel,  // Task is being executed by a sub-agent
}

/// A single task in the plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: usize,
    pub description: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub subtasks: Vec<PlanTask>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub can_parallelize: bool,
}

/// The execution plan
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionPlan {
    pub goal: String,
    pub tasks: Vec<PlanTask>,
    pub current_task_id: Option<usize>,
}

impl ExecutionPlan {
    pub fn new(goal: &str) -> Self {
        Self {
            goal: goal.to_string(),
            tasks: Vec::new(),
            current_task_id: None,
        }
    }

    pub fn add_task(&mut self, description: &str, can_parallelize: bool) -> usize {
        let id = self.tasks.len() + 1;
        self.tasks.push(PlanTask {
            id,
            description: description.to_string(),
            status: TaskStatus::Pending,
            subtasks: Vec::new(),
            result: None,
            can_parallelize,
        });
        id
    }

    pub fn update_task_status(&mut self, id: usize, status: TaskStatus) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.status = status;
        }
    }

    pub fn set_task_result(&mut self, id: usize, result: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.result = Some(result);
            task.status = TaskStatus::Completed;
        }
    }

    pub fn get_next_pending(&self) -> Option<&PlanTask> {
        self.tasks.iter().find(|t| t.status == TaskStatus::Pending)
    }

    pub fn get_parallelizable_tasks(&self) -> Vec<&PlanTask> {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending && t.can_parallelize)
            .collect()
    }

    pub fn all_completed(&self) -> bool {
        self.tasks.iter().all(|t| t.status == TaskStatus::Completed || t.status == TaskStatus::Failed)
    }
}

/// A step in the agent's reasoning process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step_number: usize,
    pub timestamp: String,
    pub thought: Option<String>,
    pub tool_call: Option<ToolCallRecord>,
    pub tool_result: Option<String>,
    pub is_final: bool,
    #[serde(default)]
    pub plan: Option<ExecutionPlan>,
    #[serde(default)]
    pub parallel_results: Vec<SubAgentResult>,
}

/// Result from a sub-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub task_id: usize,
    pub task_description: String,
    pub success: bool,
    pub result: String,
}

/// Record of a tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: Value,
}

/// Complete interaction record for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionLog {
    pub id: String,
    pub timestamp: String,
    pub user_query: String,
    pub steps: Vec<ReasoningStep>,
    pub final_response: Option<String>,
    pub total_steps: usize,
    pub success: bool,
    #[serde(default)]
    pub plan: Option<ExecutionPlan>,
}

/// Parsed response from model
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelResponse {
    #[serde(default)]
    thought: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    plan: Option<PlanResponse>,
    #[serde(default)]
    parallel_tasks: Option<Vec<ParallelTaskRequest>>,
}

/// Plan structure from model response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanResponse {
    goal: String,
    tasks: Vec<TaskResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskResponse {
    description: String,
    #[serde(default)]
    can_parallelize: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParallelTaskRequest {
    task_id: usize,
    tool: String,
    args: Value,
}

/// Callback for step updates
pub type StepCallback = Box<dyn Fn(&ReasoningStep) + Send + Sync>;

/// Callback for plan updates
pub type PlanCallback = Box<dyn Fn(&ExecutionPlan) + Send + Sync>;

/// The main AI agent
pub struct Agent {
    provider: Arc<dyn LlmProvider>,
    registry: Arc<ToolRegistry>,
    config: Config,
    step_callback: Option<StepCallback>,
    plan_callback: Option<PlanCallback>,
}

impl Agent {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        registry: Arc<ToolRegistry>,
        config: Config,
    ) -> Self {
        Self {
            provider,
            registry,
            config,
            step_callback: None,
            plan_callback: None,
        }
    }

    pub fn with_step_callback(mut self, callback: StepCallback) -> Self {
        self.step_callback = Some(callback);
        self
    }

    pub fn with_plan_callback(mut self, callback: PlanCallback) -> Self {
        self.plan_callback = Some(callback);
        self
    }

    /// Create the system prompt with JSON response format
    fn system_prompt(&self) -> String {
        let tools_desc = self.registry.format_for_prompt();
        let language = &self.config.general.language;

        format!(
            r#"You are Agenticus, a powerful AI assistant with access to various tools.
You MUST respond in {language}.

{tools_desc}

## RESPONSE FORMAT

You MUST respond with a JSON object. No other text outside JSON!

### First response - Create a plan:
```json
{{
  "thought": "Анализ задачи",
  "action": "create_plan",
  "plan": {{
    "goal": "Описание конечной цели",
    "tasks": [
      {{"description": "Задача 1", "can_parallelize": false}},
      {{"description": "Задача 2 (можно параллельно)", "can_parallelize": true}},
      {{"description": "Задача 3 (можно параллельно)", "can_parallelize": true}},
      {{"description": "Задача 4 - обработка результатов", "can_parallelize": false}}
    ]
  }}
}}
```

### Execute a single tool:
```json
{{
  "thought": "Выполняю задачу N",
  "action": "use_tool",
  "tool": "tool_name",
  "args": {{"param": "value"}},
  "current_task": 1
}}
```

### Execute multiple tools in parallel (for parallelizable tasks):
```json
{{
  "thought": "Запускаю параллельные задачи",
  "action": "parallel_execute",
  "parallel_tasks": [
    {{"task_id": 2, "tool": "web_fetch", "args": {{"url": "https://site1.com"}}}},
    {{"task_id": 3, "tool": "web_fetch", "args": {{"url": "https://site2.com"}}}}
  ]
}}
```

### Final answer:
```json
{{
  "thought": "Итог выполнения",
  "action": "final_answer",
  "answer": "Полный ответ пользователю"
}}
```

## CRITICAL RULES

1. **ВСЕГДА НАЧИНАЙ С ПЛАНА**: Первый ответ должен содержать action: "create_plan" с подробным списком задач.

2. **ИСПОЛЬЗУЙ ПАРАЛЛЕЛЬНОЕ ВЫПОЛНЕНИЕ**: Если несколько задач можно выполнить одновременно (например, загрузить несколько страниц), помечай их can_parallelize: true и используй action: "parallel_execute".

3. **ВЫПОЛНЯЙ ВСЕ ЗАДАЧИ**: Не останавливайся пока все задачи плана не выполнены.

4. **web_search -> web_fetch**: После поиска ОБЯЗАТЕЛЬНО загружай найденные страницы.

5. **КОНКРЕТНЫЙ РЕЗУЛЬТАТ**: В final_answer давай реальные данные, не ссылки.

## EXAMPLE: Finding latest anime

User: "Найди топ 5 последних аниме"

Step 1 - Create plan:
```json
{{
  "thought": "Нужно найти актуальную информацию об аниме",
  "action": "create_plan",
  "plan": {{
    "goal": "Найти и вернуть топ 5 последних вышедших аниме",
    "tasks": [
      {{"description": "Узнать текущую дату", "can_parallelize": false}},
      {{"description": "Найти сайты с аниме-релизами", "can_parallelize": false}},
      {{"description": "Загрузить контент с первого сайта", "can_parallelize": true}},
      {{"description": "Загрузить контент со второго сайта", "can_parallelize": true}},
      {{"description": "Проанализировать и составить список", "can_parallelize": false}}
    ]
  }}
}}
```

Step 2 - Get date:
```json
{{"thought": "Выполняю задачу 1", "action": "use_tool", "tool": "get_datetime", "args": {{}}, "current_task": 1}}
```

Step 3 - Search:
```json
{{"thought": "Выполняю задачу 2", "action": "use_tool", "tool": "web_search", "args": {{"query": "новые аниме декабрь 2024"}}, "current_task": 2}}
```

Step 4 - Parallel fetch:
```json
{{
  "thought": "Запускаю параллельную загрузку",
  "action": "parallel_execute",
  "parallel_tasks": [
    {{"task_id": 3, "tool": "web_fetch", "args": {{"url": "https://example1.com/anime"}}}},
    {{"task_id": 4, "tool": "web_fetch", "args": {{"url": "https://example2.com/releases"}}}}
  ]
}}
```

## IMPORTANT
- Output ONLY valid JSON!
- ALWAYS create a plan first
- Use parallel_execute when multiple tasks can run simultaneously
- Complete ALL tasks before final_answer"#
        )
    }

    /// Extract JSON from response text
    fn extract_json(&self, text: &str) -> Option<Value> {
        // Try to parse directly
        if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
            return Some(v);
        }

        // Try to find JSON in code blocks
        let re = Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").ok()?;
        if let Some(caps) = re.captures(text) {
            if let Ok(v) = serde_json::from_str::<Value>(&caps[1]) {
                return Some(v);
            }
        }

        // Try to find raw JSON object
        let re2 = Regex::new(r"\{[\s\S]*\}").ok()?;
        if let Some(m) = re2.find(text) {
            if let Ok(v) = serde_json::from_str::<Value>(m.as_str()) {
                return Some(v);
            }
        }

        None
    }

    /// Parse model response
    fn parse_response(&self, text: &str) -> Option<ModelResponse> {
        let json = self.extract_json(text)?;

        let plan = json.get("plan").and_then(|p| {
            Some(PlanResponse {
                goal: p.get("goal")?.as_str()?.to_string(),
                tasks: p.get("tasks")?
                    .as_array()?
                    .iter()
                    .filter_map(|t| {
                        Some(TaskResponse {
                            description: t.get("description")?.as_str()?.to_string(),
                            can_parallelize: t.get("can_parallelize").and_then(|v| v.as_bool()).unwrap_or(false),
                        })
                    })
                    .collect(),
            })
        });

        let parallel_tasks = json.get("parallel_tasks").and_then(|pt| {
            pt.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        Some(ParallelTaskRequest {
                            task_id: t.get("task_id")?.as_u64()? as usize,
                            tool: t.get("tool")?.as_str()?.to_string(),
                            args: t.get("args").cloned().unwrap_or(Value::Object(serde_json::Map::new())),
                        })
                    })
                    .collect()
            })
        });

        Some(ModelResponse {
            thought: json.get("thought").and_then(|v| v.as_str()).map(String::from),
            action: json.get("action").and_then(|v| v.as_str()).map(String::from),
            tool: json.get("tool").and_then(|v| v.as_str()).map(String::from),
            args: json.get("args").cloned(),
            answer: json.get("answer").and_then(|v| v.as_str()).map(String::from),
            plan,
            parallel_tasks,
        })
    }

    /// Execute parallel tasks using sub-agents
    async fn execute_parallel_tasks(
        &self,
        tasks: Vec<ParallelTaskRequest>,
        plan: &Arc<RwLock<ExecutionPlan>>,
    ) -> Vec<SubAgentResult> {
        let mut handles = Vec::new();

        for task in tasks {
            let registry = self.registry.clone();
            let task_id = task.task_id;
            let tool_name = task.tool.clone();
            let args = task.args.clone();

            // Update plan status
            {
                let mut p = plan.write().await;
                p.update_task_status(task_id, TaskStatus::Parallel);
            }

            // Spawn parallel task
            let handle = tokio::spawn(async move {
                let result = registry.execute(&tool_name, args).await;
                match result {
                    Ok(r) => SubAgentResult {
                        task_id,
                        task_description: tool_name,
                        success: r.success,
                        result: if r.success { r.output } else { r.error.unwrap_or_default() },
                    },
                    Err(e) => SubAgentResult {
                        task_id,
                        task_description: tool_name,
                        success: false,
                        result: e.to_string(),
                    },
                }
            });

            handles.push(handle);
        }

        // Wait for all parallel tasks to complete
        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                // Update plan with result
                {
                    let mut p = plan.write().await;
                    if result.success {
                        p.set_task_result(result.task_id, result.result.clone());
                    } else {
                        p.update_task_status(result.task_id, TaskStatus::Failed);
                    }
                }
                results.push(result);
            }
        }

        results
    }

    /// Run the agent on a user query
    pub async fn run(&self, user_query: &str) -> Result<InteractionLog> {
        let interaction_id = uuid::Uuid::new_v4().to_string();
        let start_time = Utc::now();

        info!(
            interaction_id = %interaction_id,
            query = %user_query,
            "Starting agent run"
        );

        let mut messages = vec![
            Message::system(self.system_prompt()),
            Message::user(user_query),
        ];

        let mut steps: Vec<ReasoningStep> = Vec::new();
        let mut final_response: Option<String> = None;
        let max_steps = self.config.general.max_steps;
        let plan = Arc::new(RwLock::new(ExecutionPlan::default()));

        for step_num in 1..=max_steps {
            debug!(step = step_num, "Executing step");

            // Get LLM response
            let response = self.provider.generate(&messages, None).await?;

            let response_text = response.content.clone().unwrap_or_default();
            debug!("Raw response: {}", &response_text);

            // Parse JSON response
            let parsed = match self.parse_response(&response_text) {
                Some(p) => p,
                None => {
                    warn!("Could not parse JSON response, treating as final answer");
                    let step = ReasoningStep {
                        step_number: step_num,
                        timestamp: Utc::now().to_rfc3339(),
                        thought: None,
                        tool_call: None,
                        tool_result: None,
                        is_final: true,
                        plan: Some(plan.read().await.clone()),
                        parallel_results: Vec::new(),
                    };

                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);
                    final_response = Some(response_text);
                    break;
                }
            };

            // Create step record
            let mut step = ReasoningStep {
                step_number: step_num,
                timestamp: Utc::now().to_rfc3339(),
                thought: parsed.thought.clone(),
                tool_call: None,
                tool_result: None,
                is_final: false,
                plan: None,
                parallel_results: Vec::new(),
            };

            // Handle different actions
            match parsed.action.as_deref() {
                Some("create_plan") => {
                    if let Some(plan_resp) = &parsed.plan {
                        let mut p = plan.write().await;
                        *p = ExecutionPlan::new(&plan_resp.goal);
                        for task in &plan_resp.tasks {
                            p.add_task(&task.description, task.can_parallelize);
                        }
                        drop(p);

                        step.plan = Some(plan.read().await.clone());

                        // Notify plan callback
                        if let Some(ref callback) = self.plan_callback {
                            callback(&*plan.read().await);
                        }

                        if let Some(ref callback) = self.step_callback {
                            callback(&step);
                        }
                        steps.push(step);

                        messages.push(Message::assistant(&response_text));
                        messages.push(Message::user(
                            "План создан. Теперь выполняй задачи по порядку. Используй parallel_execute для задач с can_parallelize: true когда они идут подряд. Ответь JSON."
                        ));
                    }
                }

                Some("use_tool") => {
                    let tool_name = match &parsed.tool {
                        Some(t) => t.clone(),
                        None => {
                            warn!("No tool specified in use_tool action");
                            step.is_final = true;
                            final_response = Some("Ошибка: не указан инструмент".to_string());
                            if let Some(ref callback) = self.step_callback {
                                callback(&step);
                            }
                            steps.push(step);
                            break;
                        }
                    };

                    let args = parsed.args.clone().unwrap_or(Value::Object(serde_json::Map::new()));

                    info!(tool = %tool_name, args = %args, "Executing tool");

                    step.tool_call = Some(ToolCallRecord {
                        name: tool_name.clone(),
                        arguments: args.clone(),
                    });

                    // Execute the tool
                    let result = self.registry.execute(&tool_name, args).await?;

                    let result_str = if result.success {
                        result.output
                    } else {
                        format!("Error: {}", result.error.unwrap_or_default())
                    };

                    step.tool_result = Some(result_str.clone());
                    step.plan = Some(plan.read().await.clone());

                    info!(tool = %tool_name, success = result.success, "Tool execution complete");

                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);

                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(&format!(
                        "Результат {}:\n{}\n\nПродолжай выполнение плана. Если есть параллельные задачи - используй parallel_execute. Ответь JSON.",
                        tool_name, result_str
                    )));
                }

                Some("parallel_execute") => {
                    if let Some(parallel_tasks) = parsed.parallel_tasks {
                        info!("Executing {} parallel tasks", parallel_tasks.len());

                        let results = self.execute_parallel_tasks(parallel_tasks, &plan).await;

                        step.parallel_results = results.clone();
                        step.plan = Some(plan.read().await.clone());

                        // Notify plan callback
                        if let Some(ref callback) = self.plan_callback {
                            callback(&*plan.read().await);
                        }

                        if let Some(ref callback) = self.step_callback {
                            callback(&step);
                        }
                        steps.push(step);

                        // Build consolidated results message
                        let mut results_text = String::from("Результаты параллельного выполнения:\n\n");
                        for r in &results {
                            results_text.push_str(&format!(
                                "=== Задача {} ({}) ===\n{}: {}\n{}\n\n",
                                r.task_id,
                                r.task_description,
                                if r.success { "Успех" } else { "Ошибка" },
                                r.task_description,
                                r.result
                            ));
                        }

                        messages.push(Message::assistant(&response_text));
                        messages.push(Message::user(&format!(
                            "{}\nПродолжай выполнение оставшихся задач плана или дай final_answer если всё готово. Ответь JSON.",
                            results_text
                        )));
                    }
                }

                Some("final_answer") => {
                    step.is_final = true;
                    step.plan = Some(plan.read().await.clone());
                    final_response = parsed.answer.or(parsed.thought);

                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);
                    break;
                }

                _ => {
                    step.is_final = true;
                    step.plan = Some(plan.read().await.clone());
                    final_response = parsed.answer.or(parsed.thought).or(Some(response_text));

                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);
                    break;
                }
            }
        }

        // If we hit max steps without a final response
        if final_response.is_none() && steps.len() >= max_steps {
            warn!("Reached maximum steps without final response");
            let p = plan.read().await;
            final_response = Some(format!(
                "Достиг максимального количества шагов ({}).\n\nВыполненные задачи:\n{}",
                max_steps,
                p.tasks
                    .iter()
                    .map(|t| format!(
                        "{} {} - {}",
                        match t.status {
                            TaskStatus::Completed => "✓",
                            TaskStatus::Failed => "✗",
                            TaskStatus::InProgress => "⟳",
                            TaskStatus::Parallel => "⇆",
                            TaskStatus::Pending => "○",
                        },
                        t.description,
                        t.result.as_deref().unwrap_or("нет результата")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        let log = InteractionLog {
            id: interaction_id,
            timestamp: start_time.to_rfc3339(),
            user_query: user_query.to_string(),
            steps: steps.clone(),
            final_response,
            total_steps: steps.len(),
            success: true,
            plan: Some(plan.read().await.clone()),
        };

        info!(
            steps = log.total_steps,
            success = log.success,
            "Agent run complete"
        );

        Ok(log)
    }
}
