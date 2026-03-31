use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub llm: LlmConfig,
    pub engram: EngramConfig,
    pub credd: CreddConfig,
    pub agents: AgentsConfig,
    pub tui: TuiConfig,
    pub brain: BrainConfig,
    pub session: SessionConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub model_path: String,
    pub model_name: String,
    pub server_path: String,
    pub context_length: u32,
    pub port: u16,
    pub gpu_layers: u32,
    pub temperature_casual: f32,
    pub temperature_routing: f32,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EngramConfig {
    pub url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CreddConfig {
    pub url: String,
    pub agent_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    pub claude: AgentEntry,
    pub codex: AgentEntry,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AgentEntry {
    pub command: String,
    pub args: Vec<String>,
    pub default_model: String,
    pub model_light: String,
    pub model_medium: String,
    pub model_heavy: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    pub theme: String,
    pub background_pattern: bool,
    pub animations: bool,
    pub fps: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BrainConfig {
    pub db_path: String,
    pub dimension: u32,
    pub decay_rate: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub auto_store_to_engram: bool,
    pub max_context_messages: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            llm: LlmConfig::default(),
            engram: EngramConfig::default(),
            credd: CreddConfig::default(),
            agents: AgentsConfig::default(),
            tui: TuiConfig::default(),
            brain: BrainConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            model_name: "qwen3-14b".to_string(),
            server_path: "llama-server".to_string(),
            context_length: 8192,
            port: 8080,
            gpu_layers: 99,
            temperature_casual: 0.7,
            temperature_routing: 0.3,
            base_url: None,
        }
    }
}

impl Default for EngramConfig {
    fn default() -> Self {
        Self {
            url: "http://100.64.0.13:4203".to_string(),
            api_key: String::new(),
        }
    }
}

impl Default for CreddConfig {
    fn default() -> Self {
        Self {
            url: "http://100.64.0.2:4400".to_string(),
            agent_key: String::new(),
        }
    }
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            claude: AgentEntry {
                command: "claude".to_string(),
                args: vec!["-p".to_string()],
                default_model: "opus".to_string(),
                model_light: "claude-haiku-4-5-20251001".to_string(),
                model_medium: "claude-sonnet-4-6".to_string(),
                model_heavy: "claude-opus-4-6".to_string(),
            },
            codex: AgentEntry {
                command: "codex".to_string(),
                args: vec!["-q".to_string()],
                default_model: "gpt-5.4-mini".to_string(),
                model_light: "gpt-5.1-codex-mini".to_string(),
                model_medium: "gpt-5.4-mini".to_string(),
                model_heavy: "gpt-5.4".to_string(),
            },
        }
    }
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            theme: "jujutsu".to_string(),
            background_pattern: true,
            animations: true,
            fps: 30,
        }
    }
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            db_path: String::new(),
            dimension: 512,
            decay_rate: 0.995,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_store_to_engram: true,
            max_context_messages: 50,
        }
    }
}

impl Config {
    pub fn from_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            Ok(Self::from_str(&contents)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("eidolon")
            .join("config.toml")
    }
}
