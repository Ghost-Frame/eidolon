use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 7700,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainConfig {
    pub db_path: String,
    pub data_dir: String,
}

impl Default for BrainConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/zan".to_string());
        BrainConfig {
            db_path: format!("{}/engram/data/brain.db", home),
            data_dir: format!("{}/eidolon/data", home),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngramConfig {
    pub url: String,
}

impl Default for EngramConfig {
    fn default() -> Self {
        EngramConfig {
            url: "http://localhost:4200".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    pub default_model: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            command: "claude".to_string(),
            args: vec!["-p".to_string(), "--output-format".to_string(), "stream-json".to_string()],
            models: vec!["opus".to_string(), "sonnet".to_string(), "haiku".to_string()],
            default_model: "sonnet".to_string(),
            env: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub brain: BrainConfig,
    #[serde(default)]
    pub engram: EngramConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    // Not stored in toml -- loaded from env var EIDOLON_API_KEY or toml [auth] section
    #[serde(skip)]
    pub api_key: String,
}

impl Default for Config {
    fn default() -> Self {
        let mut agents = HashMap::new();
        agents.insert("claude-code".to_string(), AgentConfig::default());
        Config {
            server: ServerConfig::default(),
            brain: BrainConfig::default(),
            engram: EngramConfig::default(),
            agents,
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    brain: BrainConfig,
    #[serde(default)]
    engram: EngramConfig,
    #[serde(default)]
    agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    auth: AuthConfig,
}

#[derive(Debug, Default, Deserialize)]
struct AuthConfig {
    api_key: Option<String>,
}

impl Config {
    pub fn default_path() -> String {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/zan".to_string());
        format!("{}/.config/eidolon/config.toml", home)
    }

    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read config {}: {}", path, e))?;

        let raw: RawConfig = toml::from_str(&content)
            .map_err(|e| format!("failed to parse config {}: {}", path, e))?;

        // API key: env var takes precedence over config file
        let api_key = std::env::var("EIDOLON_API_KEY")
            .ok()
            .or(raw.auth.api_key)
            .unwrap_or_default();

        if api_key.is_empty() {
            return Err("EIDOLON_API_KEY is required (set env var or [auth] api_key in config)".to_string());
        }

        Ok(Config {
            server: raw.server,
            brain: raw.brain,
            engram: raw.engram,
            agents: raw.agents,
            api_key,
        })
    }

    pub fn load_or_default(path: Option<&str>) -> Result<Self, String> {
        let config_path = path.map(|s| s.to_string()).unwrap_or_else(Self::default_path);

        // If config file doesn't exist, use defaults but still require API key
        let mut config = if std::path::Path::new(&config_path).exists() {
            Self::load(&config_path)?
        } else {
            tracing::warn!("config file not found at {}, using defaults", config_path);
            let api_key = std::env::var("EIDOLON_API_KEY")
                .map_err(|_| "EIDOLON_API_KEY env var required when no config file exists".to_string())?;
            if api_key.is_empty() {
                return Err("EIDOLON_API_KEY must not be empty".to_string());
            }
            let mut cfg = Config::default();
            cfg.api_key = api_key;
            cfg
        };

        // If agents map is empty, add default claude-code agent
        if config.agents.is_empty() {
            config.agents.insert("claude-code".to_string(), AgentConfig::default());
        }

        Ok(config)
    }
}
