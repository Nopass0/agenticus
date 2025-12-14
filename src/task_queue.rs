use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

use crate::config::Config;

/// A task received from the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTask {
    pub id: String,
    pub task: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Result of a completed task to send back to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub success: bool,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Message types for WebSocket communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum WsMessage {
    /// Client authentication
    #[serde(rename = "auth")]
    Auth { code: String },
    /// Auth response from server
    #[serde(rename = "auth_response")]
    AuthResponse { success: bool, message: String },
    /// New task from server
    #[serde(rename = "task")]
    Task(ServerTask),
    /// Task result from client
    #[serde(rename = "result")]
    Result(TaskResult),
    /// Heartbeat ping
    #[serde(rename = "ping")]
    Ping,
    /// Heartbeat pong
    #[serde(rename = "pong")]
    Pong,
    /// Status update
    #[serde(rename = "status")]
    Status { status: String, message: String },
    /// Error message
    #[serde(rename = "error")]
    Error { message: String },
}

/// Task queue state
#[derive(Debug)]
pub struct TaskQueueState {
    pub connected: bool,
    pub authenticated: bool,
    pub pending_tasks: Vec<ServerTask>,
    pub current_task: Option<ServerTask>,
}

impl Default for TaskQueueState {
    fn default() -> Self {
        Self {
            connected: false,
            authenticated: false,
            pending_tasks: Vec::new(),
            current_task: None,
        }
    }
}

pub type SharedTaskQueueState = Arc<RwLock<TaskQueueState>>;

/// Task queue client for connecting to the server
pub struct TaskQueueClient {
    config: Config,
    state: SharedTaskQueueState,
    result_sender: Option<mpsc::Sender<TaskResult>>,
}

impl TaskQueueClient {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(TaskQueueState::default())),
            result_sender: None,
        }
    }

    pub fn state(&self) -> SharedTaskQueueState {
        self.state.clone()
    }

    /// Send a task result back to the server
    pub async fn send_result(&self, result: TaskResult) -> Result<()> {
        if let Some(sender) = &self.result_sender {
            sender.send(result).await?;
        }
        Ok(())
    }

    /// Get the next pending task
    pub async fn get_next_task(&self) -> Option<ServerTask> {
        let mut state = self.state.write().await;
        if state.current_task.is_some() {
            return None; // Already processing a task
        }
        if let Some(task) = state.pending_tasks.pop() {
            state.current_task = Some(task.clone());
            Some(task)
        } else {
            None
        }
    }

    /// Mark current task as complete
    pub async fn complete_current_task(&self, result: TaskResult) -> Result<()> {
        {
            let mut state = self.state.write().await;
            state.current_task = None;
        }
        self.send_result(result).await
    }

    /// Start the WebSocket connection and task loop
    pub async fn run(
        &mut self,
        task_sender: mpsc::Sender<ServerTask>,
    ) -> Result<()> {
        // Copy config values to avoid borrow issues
        let enabled = self.config.server.enabled;
        let url = self.config.server.url.clone();
        let auth_code = self.config.server.auth_code.clone();
        let auto_reconnect = self.config.server.auto_reconnect;
        let reconnect_interval = self.config.server.reconnect_interval;

        if !enabled || url.is_empty() {
            info!("Task queue server not configured, skipping connection");
            return Ok(());
        }

        loop {
            match self.connect_and_run(&url, &auth_code, task_sender.clone()).await {
                Ok(_) => {
                    info!("WebSocket connection closed normally");
                }
                Err(e) => {
                    error!("WebSocket connection error: {}", e);
                }
            }

            // Update state
            {
                let mut state = self.state.write().await;
                state.connected = false;
                state.authenticated = false;
            }

            if !auto_reconnect {
                break;
            }

            info!("Reconnecting in {} seconds...", reconnect_interval);
            tokio::time::sleep(tokio::time::Duration::from_secs(reconnect_interval)).await;
        }

        Ok(())
    }

    async fn connect_and_run(
        &mut self,
        url: &str,
        auth_code: &str,
        task_sender: mpsc::Sender<ServerTask>,
    ) -> Result<()> {
        info!("Connecting to task server: {}", url);

        let (ws_stream, _) = connect_async(url).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("Connected to task server");

        // Update state
        {
            let mut state = self.state.write().await;
            state.connected = true;
        }

        // Create channel for sending results
        let (result_tx, mut result_rx) = mpsc::channel::<TaskResult>(32);
        self.result_sender = Some(result_tx);

        // Send authentication
        let auth_msg = WsMessage::Auth {
            code: auth_code.to_string(),
        };
        write
            .send(Message::Text(serde_json::to_string(&auth_msg)?))
            .await?;

        // Create heartbeat task
        let heartbeat_interval = tokio::time::Duration::from_secs(30);
        let mut heartbeat = tokio::time::interval(heartbeat_interval);

        loop {
            tokio::select! {
                // Handle incoming messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(e) = self.handle_message(&text, &task_sender).await {
                                error!("Error handling message: {}", e);
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("Server closed connection");
                            break;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            write.send(Message::Pong(data)).await?;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket stream ended");
                            break;
                        }
                        _ => {}
                    }
                }

                // Handle outgoing results
                result = result_rx.recv() => {
                    if let Some(result) = result {
                        let msg = WsMessage::Result(result);
                        write.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                    }
                }

                // Send heartbeat
                _ = heartbeat.tick() => {
                    let ping = WsMessage::Ping;
                    write.send(Message::Text(serde_json::to_string(&ping)?)).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_message(
        &self,
        text: &str,
        task_sender: &mpsc::Sender<ServerTask>,
    ) -> Result<()> {
        let msg: WsMessage = serde_json::from_str(text)?;

        match msg {
            WsMessage::AuthResponse { success, message } => {
                if success {
                    info!("Authenticated with server: {}", message);
                    let mut state = self.state.write().await;
                    state.authenticated = true;
                } else {
                    error!("Authentication failed: {}", message);
                }
            }
            WsMessage::Task(task) => {
                info!("Received task: {} - {}", task.id, task.task);

                // Add to pending queue
                {
                    let mut state = self.state.write().await;
                    state.pending_tasks.push(task.clone());
                }

                // Notify the main loop
                task_sender.send(task).await?;
            }
            WsMessage::Pong => {
                // Heartbeat response, ignore
            }
            WsMessage::Error { message } => {
                error!("Server error: {}", message);
            }
            WsMessage::Status { status, message } => {
                info!("Server status: {} - {}", status, message);
            }
            _ => {
                warn!("Unexpected message type received");
            }
        }

        Ok(())
    }
}

/// Run the task queue in background
pub fn spawn_task_queue(
    config: Config,
    task_sender: mpsc::Sender<ServerTask>,
) -> (tokio::task::JoinHandle<()>, SharedTaskQueueState) {
    let mut client = TaskQueueClient::new(config);
    let state = client.state();

    let handle = tokio::spawn(async move {
        if let Err(e) = client.run(task_sender).await {
            error!("Task queue error: {}", e);
        }
    });

    (handle, state)
}
