use crate::config::Config;
use crate::llm::{LlmProvider, Message};
use crate::tools::ToolRegistry;
use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// A step in the agent's reasoning process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step_number: usize,
    pub timestamp: String,
    pub thought: Option<String>,
    pub tool_call: Option<ToolCallRecord>,
    pub tool_result: Option<String>,
    pub is_final: bool,
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
}

/// Callback for step updates
pub type StepCallback = Box<dyn Fn(&ReasoningStep) + Send + Sync>;

/// The main AI agent
pub struct Agent {
    provider: Arc<dyn LlmProvider>,
    registry: Arc<ToolRegistry>,
    config: Config,
    step_callback: Option<StepCallback>,
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
        }
    }

    pub fn with_step_callback(mut self, callback: StepCallback) -> Self {
        self.step_callback = Some(callback);
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

When you need to use a tool:
```json
{{
  "thought": "ПЛАНИРОВАНИЕ: [общий план] | ТЕКУЩИЙ ШАГ: [что делаю сейчас] | ОСТАЛОСЬ: [что ещё нужно сделать]",
  "action": "use_tool",
  "tool": "tool_name",
  "args": {{"param1": "value1", "param2": "value2"}}
}}
```

When you have the final answer:
```json
{{
  "thought": "краткий итог",
  "action": "final_answer",
  "answer": "Полный ответ пользователю на {language}"
}}
```

## CRITICAL RULES - FOLLOW STRICTLY!

1. **ПЛАНИРУЙ ПЕРЕД ДЕЙСТВИЕМ**: Перед первым шагом составь план в thought. Разбей задачу на подзадачи.

2. **ВЫПОЛНЯЙ ЗАДАЧУ ПОЛНОСТЬЮ**:
   - НЕ ОСТАНАВЛИВАЙСЯ на полпути!
   - Если нашёл ссылки через web_search - ОБЯЗАТЕЛЬНО загрузи их через web_fetch чтобы получить реальный контент
   - Если нужна дата/время - СНАЧАЛА вызови get_datetime
   - Используй СТОЛЬКО шагов, СКОЛЬКО НУЖНО для полного решения

3. **НЕ ДАВАЙ ЧАСТИЧНЫЕ ОТВЕТЫ**:
   - ПЛОХО: "Вот ссылки на сайты где можно найти..."
   - ХОРОШО: Реальный контент, данные, информация из источников

4. **ИСПОЛЬЗУЙ ИНСТРУМЕНТЫ АКТИВНО**:
   - web_search: найти ссылки
   - web_fetch: ОБЯЗАТЕЛЬНО загрузить контент найденных страниц!
   - get_datetime: если контекст требует знания даты
   - НЕ спрашивай разрешения - ДЕЙСТВУЙ!

5. **АНАЛИЗИРУЙ РЕЗУЛЬТАТЫ**: После каждого инструмента думай: "Достаточно ли информации? Нужны ли ещё данные?"

## ПРИМЕРЫ ПРАВИЛЬНОГО ПЛАНИРОВАНИЯ

User: "Какие новые аниме вышли?"
```json
{{"thought": "ПЛАНИРОВАНИЕ: 1) Узнать текущую дату 2) Найти сайты с аниме 3) Загрузить контент 4) Извлечь список | ТЕКУЩИЙ ШАГ: Узнаю дату | ОСТАЛОСЬ: поиск, загрузка, анализ", "action": "use_tool", "tool": "get_datetime", "args": {{}}}}
```
После получения даты:
```json
{{"thought": "ПЛАНИРОВАНИЕ: продолжаю | ТЕКУЩИЙ ШАГ: Ищу сайты с аниме | ОСТАЛОСЬ: загрузить контент, извлечь список", "action": "use_tool", "tool": "web_search", "args": {{"query": "новые аниме декабрь 2024 список"}}}}
```
После поиска:
```json
{{"thought": "ПЛАНИРОВАНИЕ: продолжаю | ТЕКУЩИЙ ШАГ: Загружаю контент первого сайта | ОСТАЛОСЬ: возможно загрузить ещё, сформировать ответ", "action": "use_tool", "tool": "web_fetch", "args": {{"url": "https://example.com/anime-list"}}}}
```

User: "Открой блокнот и напиши там привет"
```json
{{"thought": "ПЛАНИРОВАНИЕ: 1) Открыть блокнот 2) Подождать 3) Ввести текст | ТЕКУЩИЙ ШАГ: Открываю блокнот | ОСТАЛОСЬ: ввести текст", "action": "use_tool", "tool": "launch_app", "args": {{"app": "notepad"}}}}
```
После открытия:
```json
{{"thought": "ПЛАНИРОВАНИЕ: продолжаю | ТЕКУЩИЙ ШАГ: Ввожу текст | ОСТАЛОСЬ: ничего", "action": "use_tool", "tool": "type_text", "args": {{"text": "привет"}}}}
```

## ВАЖНО

- НИКОГДА не давай final_answer пока задача не решена ПОЛНОСТЬЮ
- Если web_search дал ссылки - ЗАГРУЗИ их через web_fetch!
- Если задача требует нескольких действий - ВЫПОЛНИ ИХ ВСЕ
- Output ONLY valid JSON! No markdown, no explanations outside JSON!"#
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

        Some(ModelResponse {
            thought: json.get("thought").and_then(|v| v.as_str()).map(String::from),
            action: json.get("action").and_then(|v| v.as_str()).map(String::from),
            tool: json.get("tool").and_then(|v| v.as_str()).map(String::from),
            args: json.get("args").cloned(),
            answer: json.get("answer").and_then(|v| v.as_str()).map(String::from),
        })
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

        for step_num in 1..=max_steps {
            debug!(step = step_num, "Executing step");

            // Get LLM response (no tools passed - we use JSON parsing)
            let response = self.provider.generate(&messages, None).await?;

            let response_text = response.content.clone().unwrap_or_default();
            debug!("Raw response: {}", &response_text);

            // Parse JSON response
            let parsed = match self.parse_response(&response_text) {
                Some(p) => p,
                None => {
                    // If we can't parse JSON, treat the response as final answer
                    warn!("Could not parse JSON response, treating as final answer");
                    let mut step = ReasoningStep {
                        step_number: step_num,
                        timestamp: Utc::now().to_rfc3339(),
                        thought: None,
                        tool_call: None,
                        tool_result: None,
                        is_final: true,
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
            };

            // Check action type
            match parsed.action.as_deref() {
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

                    info!(
                        tool = %tool_name,
                        args = %args,
                        "Executing tool"
                    );

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

                    info!(
                        tool = %tool_name,
                        success = result.success,
                        "Tool execution complete"
                    );

                    // Notify callback
                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);

                    // Add to conversation for next iteration
                    messages.push(Message::assistant(&response_text));
                    messages.push(Message::user(&format!(
                        "Tool result for {}:\n{}\n\nПРОДОЛЖАЙ ВЫПОЛНЕНИЕ ПЛАНА! Проанализируй результат:\n- Задача решена ПОЛНОСТЬЮ? Если да - дай final_answer с полным ответом.\n- Нужны ещё данные? Используй следующий инструмент по плану.\n- Если web_search вернул ссылки - ЗАГРУЗИ контент через web_fetch!\nОтветь JSON.",
                        tool_name, result_str
                    )));
                }
                Some("final_answer") => {
                    step.is_final = true;
                    final_response = parsed.answer.or(parsed.thought);

                    if let Some(ref callback) = self.step_callback {
                        callback(&step);
                    }
                    steps.push(step);
                    break;
                }
                _ => {
                    // Unknown action or no action - treat as final
                    step.is_final = true;
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
            final_response = Some(format!(
                "Достиг максимального количества шагов ({}). Вот что я узнал:\n{}",
                max_steps,
                steps
                    .iter()
                    .filter_map(|s| s.tool_result.clone())
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
        };

        info!(
            steps = log.total_steps,
            success = log.success,
            "Agent run complete"
        );

        Ok(log)
    }
}
