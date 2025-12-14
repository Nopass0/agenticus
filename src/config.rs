use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub openrouter: OpenRouterConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Which provider to use: "ollama" or "openrouter"
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Language for AI responses
    #[serde(default = "default_language")]
    pub language: String,
    /// Maximum reasoning steps
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub url: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterConfig {
    #[serde(default = "default_openrouter_url")]
    pub url: String,
    #[serde(default = "default_openrouter_model")]
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_log_level")]
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_memory_file")]
    pub file: String,
}

// Default functions
fn default_provider() -> String {
    "openrouter".to_string()
}

fn default_language() -> String {
    "русский".to_string()
}

fn default_max_steps() -> usize {
    10
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_model() -> String {
    "qwen2.5:8b".to_string()
}

fn default_openrouter_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}

fn default_openrouter_model() -> String {
    "google/gemini-2.0-flash-lite-001".to_string()
}

fn default_log_dir() -> String {
    "logs".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_memory_file() -> String {
    "memory.json".to_string()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            language: default_language(),
            max_steps: default_max_steps(),
        }
    }
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            url: default_ollama_url(),
            model: default_ollama_model(),
        }
    }
}

impl Default for OpenRouterConfig {
    fn default() -> Self {
        Self {
            url: default_openrouter_url(),
            model: default_openrouter_model(),
            api_key: String::new(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_dir: default_log_dir(),
            level: default_log_level(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            file: default_memory_file(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            ollama: OllamaConfig::default(),
            openrouter: OpenRouterConfig::default(),
            logging: LoggingConfig::default(),
            memory: MemoryConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(&config_path, content)?;
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agenticus")
            .join("config.toml")
    }

    pub fn data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agenticus")
    }
}
