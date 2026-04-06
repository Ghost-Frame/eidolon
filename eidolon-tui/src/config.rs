use serde::{Deserialize, Serialize};
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
    pub embedding: EmbeddingConfig,
    pub daemon: DaemonConfig,
    pub claude: ClaudeConfig,
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
    pub panel_split: u16,
    pub mouse_enabled: bool,
    pub default_input_target: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub spawn_method: String,
    pub cli_path: String,
    pub default_model: String,
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

/// Embedding provider configuration -- mirrors eidolon-daemon's EmbeddingConfig.
/// Default: Engram (uses the [engram] section's URL and API key).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum EmbeddingConfig {
    Engram {
        #[serde(default = "default_embed_dim")]
        dim: usize,
    },
    Openai {
        #[serde(default)]
        model: Option<String>,
        #[serde(default = "default_openai_dim")]
        dim: usize,
    },
    Http {
        url: String,
        #[serde(default = "default_embed_dim")]
        dim: usize,
        #[serde(default)]
        auth_header: Option<String>,
    },
}

fn default_embed_dim() -> usize { 1024 }
fn default_openai_dim() -> usize { 1536 }

impl Default for EmbeddingConfig {
    fn default() -> Self {
        EmbeddingConfig::Engram { dim: 1024 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub url: String,
    pub api_key: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:7700".to_string(),
            api_key: String::new(),
        }
    }
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
            embedding: EmbeddingConfig::default(),
            daemon: DaemonConfig::default(),
            claude: ClaudeConfig::default(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            model_name: "mistral-nemo".to_string(),
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
            url: "http://localhost:4200".to_string(),
            api_key: String::new(),
        }
    }
}

impl Default for CreddConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:4400".to_string(),
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
            panel_split: 50,
            mouse_enabled: true,
            default_input_target: "tui".to_string(),
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            spawn_method: "cli".to_string(),
            cli_path: "claude".to_string(),
            default_model: "opus".to_string(),
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

    /// Fetch engram API key from credd at startup.
    /// Only fetches if credd.agent_key is set and engram.api_key is empty.
    pub async fn bootstrap_from_credd(&mut self) -> Result<(), String> {
        if self.credd.agent_key.is_empty() {
            return Ok(());
        }
        if !self.engram.api_key.is_empty() {
            return Ok(());
        }

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("http client: {}", e))?;

        let url = format!("{}/secret/engram/api-key-eidolon-tui", self.credd.url);
        let resp = http.get(&url)
            .header("Authorization", format!("Bearer {}", self.credd.agent_key))
            .send()
            .await
            .map_err(|e| format!("credd fetch: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("credd returned HTTP {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await
            .map_err(|e| format!("credd parse: {}", e))?;

        let key = body.get("value")
            .and_then(|v| v.get("key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "failed to extract key from credd response".to_string())?;

        self.engram.api_key = key.to_string();
        Ok(())
    }

    /// Warn if config file permissions are too open (unix only).
    #[cfg(unix)]
    pub fn check_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(Self::config_path()) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                eprintln!(
                    "[eidolon-tui] WARNING: config.toml has mode {:o} - recommend chmod 600",
                    mode
                );
            }
        }
    }
}

/// Build an embedding provider from the config.
/// Returns None if the provider can't be constructed (e.g. missing API key).
pub fn build_embed_provider(config: &Config) -> Option<std::sync::Arc<dyn crate::embedding::AsyncEmbeddingProvider>> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .ok()?;

    match &config.embedding {
        EmbeddingConfig::Engram { dim } => {
            let api_key = if config.engram.api_key.is_empty() {
                None
            } else {
                Some(config.engram.api_key.clone())
            };
            let provider = crate::embedding::engram::EngramProvider::new(
                http,
                config.engram.url.clone(),
                api_key,
                *dim,
            );
            Some(std::sync::Arc::new(provider))
        }
        EmbeddingConfig::Openai { model, dim } => {
            let api_key = std::env::var("OPENAI_API_KEY").ok()?;
            let provider = crate::embedding::openai::OpenaiProvider::new(
                http,
                api_key,
                model.clone(),
                *dim,
            );
            Some(std::sync::Arc::new(provider))
        }
        EmbeddingConfig::Http { url, dim, auth_header } => {
            let provider = crate::embedding::http::HttpProvider::new(
                http,
                url.clone(),
                *dim,
                auth_header.clone(),
            );
            Some(std::sync::Arc::new(provider))
        }
    }
}

/// Resolve short model aliases to full model IDs.
pub fn resolve_model_alias(input: &str) -> String {
    match input.to_lowercase().as_str() {
        // Claude
        "haiku" => "claude-haiku-4-5-20251001".to_string(),
        "sonnet" => "claude-sonnet-4-6".to_string(),
        "opus" => "claude-opus-4-6".to_string(),
        // Codex / OpenAI
        "5.4" => "gpt-5.4".to_string(),
        "5.4-mini" => "gpt-5.4-mini".to_string(),
        "5.3-codex" => "gpt-5.3-codex".to_string(),
        "5.2-codex" => "gpt-5.2-codex".to_string(),
        "5.2" => "gpt-5.2".to_string(),
        "5.1-max" => "gpt-5.1-codex-max".to_string(),
        "5.1-mini" => "gpt-5.1-codex-mini".to_string(),
        // Pass through anything else as-is
        other => other.to_string(),
    }
}
