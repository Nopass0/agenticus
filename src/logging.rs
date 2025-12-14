use crate::agent::InteractionLog;
use crate::config::Config;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize the logging system
pub fn init_logging(config: &Config) -> Result<Option<WorkerGuard>> {
    let log_dir = PathBuf::from(&config.logging.log_dir);
    fs::create_dir_all(&log_dir)?;

    let log_level = &config.logging.level;

    // Create file appender
    let file_appender = tracing_appender::rolling::daily(&log_dir, "agenticus.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Set up subscriber
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(false)
                .with_file(true)
                .with_line_number(true),
        )
        .init();

    Ok(Some(guard))
}

/// Logger for interaction history
pub struct InteractionLogger {
    log_dir: PathBuf,
}

impl InteractionLogger {
    pub fn new(log_dir: &str) -> Result<Self> {
        let log_dir = PathBuf::from(log_dir);
        fs::create_dir_all(&log_dir)?;

        Ok(Self { log_dir })
    }

    /// Log an interaction to a JSON file
    pub fn log_interaction(&self, log: &InteractionLog) -> Result<()> {
        let filename = format!(
            "interaction_{}.json",
            log.timestamp.replace(":", "-").replace(".", "-")
        );

        let path = self.log_dir.join(&filename);

        let json = serde_json::to_string_pretty(log)?;
        fs::write(&path, json)?;

        // Also append to the daily log
        self.append_to_daily_log(log)?;

        Ok(())
    }

    /// Append to a chronological daily log
    fn append_to_daily_log(&self, log: &InteractionLog) -> Result<()> {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let filename = format!("daily_{}.jsonl", date);
        let path = self.log_dir.join(&filename);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let json = serde_json::to_string(log)?;
        writeln!(file, "{}", json)?;

        Ok(())
    }

    /// Get all interactions from a specific date
    pub fn get_interactions_by_date(&self, date: &str) -> Result<Vec<InteractionLog>> {
        let filename = format!("daily_{}.jsonl", date);
        let path = self.log_dir.join(&filename);

        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)?;
        let logs: Vec<InteractionLog> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        Ok(logs)
    }

    /// Get recent interactions
    pub fn get_recent_interactions(&self, limit: usize) -> Result<Vec<InteractionLog>> {
        let mut all_logs = Vec::new();

        // Read all daily log files
        for entry in fs::read_dir(&self.log_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Ok(content) = fs::read_to_string(&path) {
                    for line in content.lines() {
                        if let Ok(log) = serde_json::from_str::<InteractionLog>(line) {
                            all_logs.push(log);
                        }
                    }
                }
            }
        }

        // Sort by timestamp (newest first)
        all_logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Take the limit
        all_logs.truncate(limit);

        Ok(all_logs)
    }

    /// Search interactions by query
    pub fn search_interactions(&self, query: &str) -> Result<Vec<InteractionLog>> {
        let query_lower = query.to_lowercase();
        let all_logs = self.get_recent_interactions(1000)?;

        let matching: Vec<_> = all_logs
            .into_iter()
            .filter(|log| {
                log.user_query.to_lowercase().contains(&query_lower)
                    || log
                        .final_response
                        .as_ref()
                        .map(|r| r.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .collect();

        Ok(matching)
    }
}

/// Format a log entry for display
pub fn format_log_entry(log: &InteractionLog) -> String {
    let mut output = String::new();

    output.push_str(&format!("ID: {}\n", log.id));
    output.push_str(&format!("Time: {}\n", log.timestamp));
    output.push_str(&format!("Query: {}\n", log.user_query));
    output.push_str(&format!("Steps: {}\n", log.total_steps));
    output.push_str(&format!("Success: {}\n", log.success));

    if let Some(response) = &log.final_response {
        let preview = if response.len() > 200 {
            format!("{}...", &response[..200])
        } else {
            response.clone()
        };
        output.push_str(&format!("Response: {}\n", preview));
    }

    output
}
