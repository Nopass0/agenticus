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
    Parallel,
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
    #[serde(default)]
    pub collected_data: Vec<String>,
}

impl ExecutionPlan {
    pub fn new(goal: &str) -> Self {
        Self {
            goal: goal.to_string(),
            tasks: Vec::new(),
            current_task_id: None,
            collected_data: Vec::new(),
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

    pub fn insert_task_after(&mut self, after_id: usize, description: &str, can_parallelize: bool) -> usize {
        let new_id = self.tasks.len() + 1;
        let pos = self.tasks.iter().position(|t| t.id == after_id).map(|p| p + 1).unwrap_or(self.tasks.len());
        self.tasks.insert(pos, PlanTask {
            id: new_id,
            description: description.to_string(),
            status: TaskStatus::Pending,
            subtasks: Vec::new(),
            result: None,
            can_parallelize,
        });
        new_id
    }

    pub fn update_task_status(&mut self, id: usize, status: TaskStatus) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.status = status;
        }
    }

    pub fn update_task_description(&mut self, id: usize, description: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.description = description.to_string();
        }
    }

    pub fn set_task_result(&mut self, id: usize, result: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            // Don't accept fake results
            if !result.is_empty() && result != "Выполнено" && result != "Done" && result.len() > 10 {
                task.result = Some(result.clone());
                task.status = TaskStatus::Completed;
                // Store as collected data for context
                self.collected_data.push(result);
            }
        }
    }

    pub fn fail_task(&mut self, id: usize, error: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.result = Some(error);
            task.status = TaskStatus::Failed;
        }
    }

    pub fn get_pending_tasks(&self) -> Vec<&PlanTask> {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Pending).collect()
    }

    pub fn get_incomplete_tasks(&self) -> Vec<&PlanTask> {
        self.tasks.iter().filter(|t| t.status != TaskStatus::Completed).collect()
    }

    pub fn all_completed(&self) -> bool {
        self.tasks.iter().all(|t| t.status == TaskStatus::Completed)
    }

    pub fn has_pending(&self) -> bool {
        self.tasks.iter().any(|t| t.status == TaskStatus::Pending)
    }

    pub fn format_status(&self) -> String {
        let mut s = String::new();
        for t in &self.tasks {
            let icon = match t.status {
                TaskStatus::Completed => "✅",
                TaskStatus::Failed => "❌",
                TaskStatus::InProgress => "🔄",
                TaskStatus::Parallel => "⚡",
                TaskStatus::Pending => "⏳",
            };
            let result_preview = t.result.as_ref().map(|r| {
                let preview = if r.len() > 50 { &r[..50] } else { r };
                format!(" → {}", preview)
            }).unwrap_or_default();
            s.push_str(&format!("{} [{}] {}{}\n", icon, t.id, t.description, result_preview));
        }
        s
    }

    pub fn get_collected_context(&self) -> String {
        if self.collected_data.is_empty() {
            String::new()
        } else {
            format!(
                "\n## COLLECTED DATA FROM PREVIOUS STEPS (USE THIS!):\n{}\n",
                self.collected_data.iter()
                    .enumerate()
                    .map(|(i, d)| {
                        let preview = if d.len() > 500 { &d[..500] } else { d };
                        format!("{}. {}", i + 1, preview)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ModelResponse {
    thought: Option<String>,
    action: Option<String>,
    tool: Option<String>,
    args: Option<Value>,
    answer: Option<String>,
    plan: Option<PlanResponse>,
    parallel_tasks: Option<Vec<ParallelTaskRequest>>,
    current_task: Option<usize>,
    // Plan modification fields
    add_tasks: Option<Vec<TaskResponse>>,
    complete_task: Option<CompleteTaskRequest>,
    fail_task: Option<FailTaskRequest>,
    update_task: Option<UpdateTaskRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompleteTaskRequest {
    task_id: usize,
    result: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailTaskRequest {
    task_id: usize,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateTaskRequest {
    task_id: usize,
    description: String,
}

/// Callback types
pub type StepCallback = Box<dyn Fn(&ReasoningStep) + Send + Sync>;
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

    /// Gather system context (date/time, OS info, owner from memory)
    async fn gather_context(&self) -> String {
        let mut context_parts = Vec::new();

        // Get current date/time
        if let Ok(result) = self.registry.execute("get_datetime", serde_json::json!({})).await {
            if result.success {
                context_parts.push(format!("📅 Текущая дата и время: {}", result.output.trim()));
            }
        }

        // Get system info
        if let Ok(result) = self.registry.execute("get_system_info", serde_json::json!({})).await {
            if result.success {
                context_parts.push(format!("💻 Системная информация:\n{}", result.output.trim()));
            }
        }

        // Try to get owner info from memory
        if let Ok(result) = self.registry.execute("memory_recall", serde_json::json!({"key": "owner"})).await {
            if result.success && !result.output.contains("not found") && !result.output.is_empty() {
                context_parts.push(format!("👤 Владелец: {}", result.output.trim()));
            }
        }

        // Try to get user name from memory
        if let Ok(result) = self.registry.execute("memory_recall", serde_json::json!({"key": "user_name"})).await {
            if result.success && !result.output.contains("not found") && !result.output.is_empty() {
                context_parts.push(format!("👤 Имя пользователя: {}", result.output.trim()));
            }
        }

        // Try to get preferences from memory
        if let Ok(result) = self.registry.execute("memory_recall", serde_json::json!({"key": "preferences"})).await {
            if result.success && !result.output.contains("not found") && !result.output.is_empty() {
                context_parts.push(format!("⚙️ Предпочтения: {}", result.output.trim()));
            }
        }

        if context_parts.is_empty() {
            String::new()
        } else {
            format!("## ТЕКУЩИЙ КОНТЕКСТ\n\n{}\n\n---\n\n", context_parts.join("\n\n"))
        }
    }

    fn system_prompt(&self, context: &str, collected_data: &str) -> String {
        let tools_desc = self.registry.format_for_prompt();
        let language = &self.config.general.language;

        let mut prompt = String::new();

        // Add context if present
        prompt.push_str(context);

        // Add collected data from previous steps
        prompt.push_str(collected_data);

        // Main instructions
        prompt.push_str(&format!(
            "You are Agenticus, a powerful AI assistant that ALWAYS completes tasks fully.\n\
            You MUST respond in {}.\n\n\
            {}\n\n",
            language, tools_desc
        ));

        // Response format - using raw string to avoid escaping issues
        prompt.push_str(r#"## RESPONSE FORMAT - ONLY JSON!

### 1. Create initial plan (ALWAYS first):
```json
{
  "thought": "Analyzing task and creating plan",
  "action": "create_plan",
  "plan": {
    "goal": "End goal description",
    "tasks": [
      {"description": "Task 1", "can_parallelize": false},
      {"description": "Task 2", "can_parallelize": true}
    ]
  }
}
```

### 2. Execute tool for a specific task:
```json
{
  "thought": "Executing task N",
  "action": "use_tool",
  "current_task": 1,
  "tool": "tool_name",
  "args": {"param": "value"}
}
```

### 3. Execute parallel tasks:
```json
{
  "thought": "Running parallel tasks",
  "action": "parallel_execute",
  "parallel_tasks": [
    {"task_id": 2, "tool": "web_fetch", "args": {"url": "..."}},
    {"task_id": 3, "tool": "web_fetch", "args": {"url": "..."}}
  ]
}
```

### 4. Modify plan (add new tasks):
```json
{
  "thought": "Need to add tasks to plan",
  "action": "modify_plan",
  "add_tasks": [
    {"description": "New task", "can_parallelize": false}
  ]
}
```

### 5. Mark task as complete (ONLY with real result):
```json
{"thought": "Task done with real data", "action": "complete_task", "complete_task": {"task_id": 1, "result": "Actual result data - NOT just 'Выполнено'!"}}
```

### 5b. Mark task as failed:
```json
{"thought": "Task failed", "action": "fail_task", "fail_task": {"task_id": 1, "reason": "Error reason"}}
```

### 6. Final answer (ONLY when ALL tasks are done):
```json
{"thought": "All tasks completed", "action": "final_answer", "answer": "Full result"}
```

## CRITICAL RULES:

1. **START WITH PLAN**: First response = create_plan

2. **SPECIFY current_task**: When using use_tool, ALWAYS specify task number

3. **USE PREVIOUS RESULTS!**
   - Data from previous steps is shown in COLLECTED DATA section above
   - USE this data! Don't search for the same info again!
   - If you found games/items in step 1, use that list in step 2!
   - READ the collected data before making new requests!

4. **NO final_answer UNTIL ALL TASKS ARE DONE!**
   - Check status: pending, in_progress, completed, failed
   - final_answer ONLY when ALL tasks = completed

5. **MODIFY PLAN**: If needed - add new tasks via modify_plan

6. **WEB SEQUENCE** (ОБЯЗАТЕЛЬНО!):
   - web_search -> найти URL
   - web_fetch -> загрузить контент с КОНКРЕТНОГО URL из результатов поиска
   - ПРОВЕРЬ что контент НЕ пустой!
   - write_file -> сохранить РЕАЛЬНЫЕ данные (НЕ placeholder!)
   - open_with_default -> открыть

7. **PARALLELISM**: Use parallel_execute for multiple web_fetch

8. **НИКОГДА НЕ ПИШИ PLACEHOLDER!**
   - НЕ пиши "информация будет позже"
   - НЕ пиши "данные будут добавлены"
   - НЕ пиши "TODO" или "..."
   - Пиши ТОЛЬКО реальные данные из результатов!
   - Если данных нет - сначала получи их!

9. **complete_task REQUIRES REAL RESULT**:
   - "result" field must contain ACTUAL data (>10 chars)
   - NOT just "Выполнено" or "Done"
   - Include real information from the task!

10. **ПРОВЕРКА РЕЗУЛЬТАТОВ**:
   - Если web_fetch вернул пустой результат - попробуй другой URL
   - Если нет данных - модифицируй план и добавь шаги для получения данных
   - НЕ считай задачу выполненной без реального результата!

REMEMBER: Output ONLY valid JSON! USE COLLECTED DATA! Complete ALL tasks with REAL data!
"#);

        prompt
    }

    fn extract_json(&self, text: &str) -> Option<Value> {
        if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
            return Some(v);
        }

        let re = Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").ok()?;
        if let Some(caps) = re.captures(text) {
            if let Ok(v) = serde_json::from_str::<Value>(&caps[1]) {
                return Some(v);
            }
        }

        let re2 = Regex::new(r"\{[\s\S]*\}").ok()?;
        if let Some(m) = re2.find(text) {
            if let Ok(v) = serde_json::from_str::<Value>(m.as_str()) {
                return Some(v);
            }
        }

        None
    }

    fn parse_response(&self, text: &str) -> Option<ModelResponse> {
        let json = self.extract_json(text)?;

        let plan = json.get("plan").and_then(|p| {
            Some(PlanResponse {
                goal: p.get("goal")?.as_str()?.to_string(),
                tasks: p.get("tasks")?
                    .as_array()?
                    .iter()
                    .filter_map(|t| Some(TaskResponse {
                        description: t.get("description")?.as_str()?.to_string(),
                        can_parallelize: t.get("can_parallelize").and_then(|v| v.as_bool()).unwrap_or(false),
                    }))
                    .collect(),
            })
        });

        let parallel_tasks = json.get("parallel_tasks").and_then(|pt| {
            pt.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|t| Some(ParallelTaskRequest {
                        task_id: t.get("task_id")?.as_u64()? as usize,
                        tool: t.get("tool")?.as_str()?.to_string(),
                        args: t.get("args").cloned().unwrap_or(Value::Object(serde_json::Map::new())),
                    }))
                    .collect()
            })
        });

        let add_tasks = json.get("add_tasks").and_then(|at| {
            at.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|t| Some(TaskResponse {
                        description: t.get("description")?.as_str()?.to_string(),
                        can_parallelize: t.get("can_parallelize").and_then(|v| v.as_bool()).unwrap_or(false),
                    }))
                    .collect()
            })
        });

        let fail_task = json.get("fail_task").and_then(|ft| {
            Some(FailTaskRequest {
                task_id: ft.get("task_id")?.as_u64()? as usize,
                reason: ft.get("reason")?.as_str()?.to_string(),
            })
        });

        let update_task = json.get("update_task").and_then(|ut| {
            Some(UpdateTaskRequest {
                task_id: ut.get("task_id")?.as_u64()? as usize,
                description: ut.get("description")?.as_str()?.to_string(),
            })
        });

        // Parse complete_task - can be either object with task_id and result, or just a number
        let complete_task = json.get("complete_task").and_then(|ct| {
            if let Some(obj) = ct.as_object() {
                Some(CompleteTaskRequest {
                    task_id: obj.get("task_id")?.as_u64()? as usize,
                    result: obj.get("result").and_then(|r| r.as_str()).unwrap_or("").to_string(),
                })
            } else if let Some(id) = ct.as_u64() {
                Some(CompleteTaskRequest {
                    task_id: id as usize,
                    result: String::new(),
                })
            } else {
                None
            }
        });

        Some(ModelResponse {
            thought: json.get("thought").and_then(|v| v.as_str()).map(String::from),
            action: json.get("action").and_then(|v| v.as_str()).map(String::from),
            tool: json.get("tool").and_then(|v| v.as_str()).map(String::from),
            args: json.get("args").cloned(),
            answer: json.get("answer").and_then(|v| v.as_str()).map(String::from),
            plan,
            parallel_tasks,
            current_task: json.get("current_task").and_then(|v| v.as_u64()).map(|v| v as usize),
            add_tasks,
            complete_task,
            fail_task,
            update_task,
        })
    }

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

            {
                let mut p = plan.write().await;
                p.update_task_status(task_id, TaskStatus::Parallel);
            }

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

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                {
                    let mut p = plan.write().await;
                    if result.success {
                        p.set_task_result(result.task_id, result.result.clone());
                    } else {
                        p.fail_task(result.task_id, result.result.clone());
                    }
                }
                results.push(result);
            }
        }

        results
    }

    /// Check if a tool result is meaningful (not empty, not placeholder, not error-like)
    fn is_meaningful_result(result: &str, tool_name: &str) -> bool {
        let trimmed = result.trim();

        // Empty or very short results are not meaningful
        if trimmed.is_empty() || trimmed.len() < 5 {
            return false;
        }

        // Placeholder patterns to detect
        let placeholder_patterns = [
            "будет заполн",
            "будет добавл",
            "placeholder",
            "TODO",
            "FIXME",
            "позже",
            "later",
            "информация будет",
            "данные будут",
            "to be filled",
            "to be added",
            "...",
            "TBD",
        ];

        let lower = trimmed.to_lowercase();
        for pattern in &placeholder_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return false;
            }
        }

        // For web_fetch, check if we got actual content
        if tool_name == "web_fetch" {
            // If result is too short for a web page, it's probably empty
            if trimmed.len() < 50 {
                return false;
            }
        }

        // For write_file, check content length in the args (not result)
        // Result should indicate success
        if tool_name == "write_file" {
            // Check if file was written with actual content
            if trimmed.contains("wrote 0 bytes") || trimmed.contains("empty") {
                return false;
            }
        }

        true
    }

    fn build_status_message(plan: &ExecutionPlan) -> String {
        let pending = plan.get_pending_tasks();
        let incomplete = plan.get_incomplete_tasks();

        if plan.all_completed() {
            "✅ ВСЕ ЗАДАЧИ ВЫПОЛНЕНЫ! Теперь можешь дать final_answer.".to_string()
        } else if pending.is_empty() && !incomplete.is_empty() {
            format!(
                "⚠️ Есть незавершённые задачи:\n{}\nВыполни их или пометь как failed.",
                incomplete.iter().map(|t| format!("  - [{}] {}", t.id, t.description)).collect::<Vec<_>>().join("\n")
            )
        } else {
            format!(
                "📋 Статус плана:\n{}\n⏳ Ожидающие задачи: {}\nПродолжай выполнение!",
                plan.format_status(),
                pending.iter().map(|t| format!("[{}]", t.id)).collect::<Vec<_>>().join(", ")
            )
        }
    }

    pub async fn run(&self, user_query: &str) -> Result<InteractionLog> {
        let interaction_id = uuid::Uuid::new_v4().to_string();
        let start_time = Utc::now();

        info!(interaction_id = %interaction_id, query = %user_query, "Starting agent run");

        // Gather system context (date/time, OS info, owner from memory)
        let context = self.gather_context().await;
        debug!("System context: {}", &context);

        let plan = Arc::new(RwLock::new(ExecutionPlan::default()));

        let mut messages = vec![
            Message::system(self.system_prompt(&context, "")),
            Message::user(user_query),
        ];

        let mut steps: Vec<ReasoningStep> = Vec::new();
        let mut final_response: Option<String> = None;
        let max_steps = self.config.general.max_steps;

        for step_num in 1..=max_steps {
            debug!(step = step_num, "Executing step");

            // Update system prompt with collected data from previous steps
            let collected = plan.read().await.get_collected_context();
            if !collected.is_empty() {
                messages[0] = Message::system(self.system_prompt(&context, &collected));
            }

            let response = self.provider.generate(&messages, None).await?;
            let response_text = response.content.clone().unwrap_or_default();
            debug!("Raw response: {}", &response_text);

            let parsed = match self.parse_response(&response_text) {
                Some(p) => p,
                None => {
                    warn!("Could not parse JSON response");
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user("Ошибка: твой ответ не JSON. Ответь ТОЛЬКО JSON!"));
                    continue;
                }
            };

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

                        if let Some(ref callback) = self.plan_callback {
                            callback(&*plan.read().await);
                        }
                        if let Some(ref callback) = self.step_callback {
                            callback(&step);
                        }
                        steps.push(step);

                        let status = Self::build_status_message(&*plan.read().await);
                        messages.push(Message::assistant(&response_text));
                        messages.push(Message::user(&format!(
                            "План создан!\n{}\nВыполняй задачи по порядку, указывая current_task. Ответь JSON.",
                            status
                        )));
                    }
                }

                Some("use_tool") => {
                    let tool_name = match &parsed.tool {
                        Some(t) => t.clone(),
                        None => {
                            messages.push(Message::assistant(&response_text));
                            messages.push(Message::user("Ошибка: не указан tool. Укажи tool и args."));
                            continue;
                        }
                    };

                    let current_task = parsed.current_task;
                    let args = parsed.args.clone().unwrap_or(Value::Object(serde_json::Map::new()));

                    // Mark task as in progress
                    if let Some(task_id) = current_task {
                        let mut p = plan.write().await;
                        p.update_task_status(task_id, TaskStatus::InProgress);
                        p.current_task_id = Some(task_id);
                    }

                    info!(tool = %tool_name, args = %args, "Executing tool");

                    step.tool_call = Some(ToolCallRecord {
                        name: tool_name.clone(),
                        arguments: args.clone(),
                    });

                    // For write_file, check if content is meaningful BEFORE writing
                    if tool_name == "write_file" {
                        if let Some(content) = args.get("content").and_then(|c| c.as_str()) {
                            if !Self::is_meaningful_result(content, "content_check") {
                                // Content is placeholder - reject without executing
                                {
                                    let mut p = plan.write().await;
                                    if let Some(task_id) = current_task {
                                        p.update_task_status(task_id, TaskStatus::Pending);
                                    }
                                }

                                step.tool_result = Some("❌ ОТКАЗАНО: placeholder контент".to_string());
                                step.plan = Some(plan.read().await.clone());

                                if let Some(ref callback) = self.step_callback {
                                    callback(&step);
                                }
                                steps.push(step);

                                let status = Self::build_status_message(&*plan.read().await);
                                messages.push(Message::assistant(&response_text));
                                messages.push(Message::user(&format!(
                                    "❌ ОТКАЗАНО! Контент содержит placeholder или пустой!\n\
                                    Сначала получи РЕАЛЬНЫЕ данные (web_search -> web_fetch),\n\
                                    затем напиши их в файл.\n\
                                    НЕ пиши 'информация будет позже' или подобное!\n\n{}\nОтветь JSON.",
                                    status
                                )));
                                continue;
                            }
                        }
                    }

                    let result = self.registry.execute(&tool_name, args.clone()).await?;

                    let result_str = if result.success {
                        result.output.clone()
                    } else {
                        format!("Error: {}", result.error.clone().unwrap_or_default())
                    };

                    // Check if result is meaningful (not empty or placeholder)
                    let is_result_meaningful = Self::is_meaningful_result(&result_str, &tool_name);

                    // Mark task as completed or failed
                    let task_feedback;
                    if let Some(task_id) = current_task {
                        let mut p = plan.write().await;
                        if result.success && is_result_meaningful {
                            // Truncate result for storage (UTF-8 safe)
                            let short_result = if result_str.len() > 200 {
                                let truncate_at = result_str
                                    .char_indices()
                                    .take_while(|(i, _)| *i < 200)
                                    .last()
                                    .map(|(i, c)| i + c.len_utf8())
                                    .unwrap_or(0);
                                format!("{}...", &result_str[..truncate_at])
                            } else {
                                result_str.clone()
                            };
                            p.set_task_result(task_id, short_result);
                            task_feedback = format!("✅ Задача {} выполнена успешно.", task_id);
                        } else if result.success && !is_result_meaningful {
                            // Result is empty/useless - keep task pending
                            p.update_task_status(task_id, TaskStatus::Pending);
                            task_feedback = format!(
                                "⚠️ Результат ПУСТОЙ или БЕСПОЛЕЗНЫЙ! Задача {} НЕ ВЫПОЛНЕНА.\n\
                                Попробуй другой подход:\n\
                                - Используй другой URL или поисковый запрос\n\
                                - Измени параметры инструмента\n\
                                - Не пиши placeholder в файл, только реальные данные!",
                                task_id
                            );
                        } else {
                            p.fail_task(task_id, result_str.clone());
                            task_feedback = format!("❌ Задача {} провалилась: {}", task_id, result_str);
                        }
                    } else {
                        task_feedback = String::new();
                    }

                    step.tool_result = Some(result_str.clone());
                    step.plan = Some(plan.read().await.clone());

                    info!(tool = %tool_name, success = result.success, meaningful = is_result_meaningful, "Tool execution complete");

                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);

                    let status = Self::build_status_message(&*plan.read().await);
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(&format!(
                        "Результат {}:\n{}\n\n{}\n\n{}\nОтветь JSON.",
                        tool_name, result_str, task_feedback, status
                    )));
                }

                Some("parallel_execute") => {
                    if let Some(parallel_tasks) = parsed.parallel_tasks {
                        info!("Executing {} parallel tasks", parallel_tasks.len());

                        let results = self.execute_parallel_tasks(parallel_tasks, &plan).await;

                        step.parallel_results = results.clone();
                        step.plan = Some(plan.read().await.clone());

                        if let Some(ref callback) = self.plan_callback {
                            callback(&*plan.read().await);
                        }
                        if let Some(ref callback) = self.step_callback {
                            callback(&step);
                        }
                        steps.push(step);

                        let mut results_text = String::from("Результаты параллельного выполнения:\n");
                        for r in &results {
                            // UTF-8 safe truncation
                            let short_result = if r.result.len() > 300 {
                                let truncate_at = r.result
                                    .char_indices()
                                    .take_while(|(i, _)| *i < 300)
                                    .last()
                                    .map(|(i, c)| i + c.len_utf8())
                                    .unwrap_or(0);
                                format!("{}...", &r.result[..truncate_at])
                            } else {
                                r.result.clone()
                            };
                            results_text.push_str(&format!(
                                "\n[{}] {}: {}\n{}\n",
                                r.task_id,
                                if r.success { "✅" } else { "❌" },
                                r.task_description,
                                short_result
                            ));
                        }

                        let status = Self::build_status_message(&*plan.read().await);
                        messages.push(Message::assistant(&response_text));
                        messages.push(Message::user(&format!(
                            "{}\n\n{}\nОтветь JSON.",
                            results_text, status
                        )));
                    }
                }

                Some("modify_plan") => {
                    if let Some(add_tasks) = &parsed.add_tasks {
                        let mut p = plan.write().await;
                        for task in add_tasks {
                            p.add_task(&task.description, task.can_parallelize);
                        }
                    }

                    step.plan = Some(plan.read().await.clone());
                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);

                    let status = Self::build_status_message(&*plan.read().await);
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(&format!(
                        "План обновлён!\n{}\nПродолжай выполнение. Ответь JSON.",
                        status
                    )));
                }

                Some("complete_task") => {
                    if let Some(ct) = &parsed.complete_task {
                        // Validate that result is not empty or fake
                        if ct.result.is_empty() || ct.result == "Выполнено" || ct.result == "Done" || ct.result.len() < 10 {
                            messages.push(Message::assistant(&response_text));
                            messages.push(Message::user(
                                "❌ ОТКАЗАНО! complete_task требует РЕАЛЬНЫЙ результат в поле 'result'!\n\
                                НЕ просто 'Выполнено' или 'Done'!\n\
                                Включи настоящую информацию из задачи.\nОтветь JSON."
                            ));
                            continue;
                        }

                        let mut p = plan.write().await;
                        p.set_task_result(ct.task_id, ct.result.clone());
                    }

                    step.plan = Some(plan.read().await.clone());
                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);

                    let status = Self::build_status_message(&*plan.read().await);
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(&format!("{}\nОтветь JSON.", status)));
                }

                Some("fail_task") => {
                    if let Some(ft) = &parsed.fail_task {
                        let mut p = plan.write().await;
                        p.fail_task(ft.task_id, ft.reason.clone());
                    }

                    step.plan = Some(plan.read().await.clone());
                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);

                    let status = Self::build_status_message(&*plan.read().await);
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(&format!("{}\nОтветь JSON.", status)));
                }

                Some("final_answer") => {
                    let p = plan.read().await;

                    // Check if all tasks are complete
                    if !p.all_completed() && p.has_pending() {
                        let pending: Vec<_> = p.get_pending_tasks().iter().map(|t| format!("[{}] {}", t.id, t.description)).collect();
                        drop(p);

                        messages.push(Message::assistant(&response_text));
                        messages.push(Message::user(&format!(
                            "❌ НЕЛЬЗЯ дать final_answer! Есть невыполненные задачи:\n{}\n\nВыполни их сначала! Ответь JSON.",
                            pending.join("\n")
                        )));
                        continue;
                    }
                    drop(p);

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
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(
                        "Неизвестное действие. Используй: create_plan, use_tool, parallel_execute, modify_plan, complete_task, fail_task, final_answer. Ответь JSON."
                    ));
                    continue;
                }
            }
        }

        if final_response.is_none() && steps.len() >= max_steps {
            warn!("Reached maximum steps without final response");
            let p = plan.read().await;
            final_response = Some(format!(
                "Достиг максимального количества шагов ({}).\n\nСтатус плана:\n{}",
                max_steps,
                p.format_status()
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

        info!(steps = log.total_steps, success = log.success, "Agent run complete");

        Ok(log)
    }
}
