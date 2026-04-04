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
    #[serde(default = "default_dream_interval")]
    pub dream_interval_secs: u64,
}

fn default_dream_interval() -> u64 {
    3600
}

impl Default for BrainConfig {
    fn default() -> Self {
        let home = default_home();
        BrainConfig {
            db_path: format!("{}/engram/data/brain.db", home),
            data_dir: format!("{}/eidolon/data", home),
            dream_interval_secs: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngramConfig {
    pub url: String,
    pub api_key: Option<String>,
    pub axon_url: Option<String>,
}

impl Default for EngramConfig {
    fn default() -> Self {
        EngramConfig {
            url: "http://localhost:4200".to_string(),
            api_key: None,
            axon_url: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawEngramConfig {
    #[serde(default = "default_engram_url")]
    url: String,
    api_key: Option<String>,
    axon_url: Option<String>,
}

fn default_engram_url() -> String {
    "http://localhost:4200".to_string()
}

impl Default for RawEngramConfig {
    fn default() -> Self {
        RawEngramConfig {
            url: default_engram_url(),
            api_key: None,
            axon_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreddConfig {
    pub url: String,
    pub agent_key: Option<String>,
    pub tier3_trust_threshold: u8,
}

impl Default for CreddConfig {
    fn default() -> Self {
        CreddConfig {
            url: "http://127.0.0.1:4400".to_string(),
            agent_key: None,
            tier3_trust_threshold: 80,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCreddConfig {
    #[serde(default = "default_credd_url")]
    url: String,
    agent_key: Option<String>,
    #[serde(default = "default_tier3_threshold")]
    tier3_trust_threshold: u8,
}

fn default_credd_url() -> String {
    "http://127.0.0.1:4400".to_string()
}

fn default_tier3_threshold() -> u8 {
    80
}

impl Default for RawCreddConfig {
    fn default() -> Self {
        RawCreddConfig {
            url: default_credd_url(),
            agent_key: None,
            tier3_trust_threshold: default_tier3_threshold(),
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
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            command: "claude".to_string(),
            args: vec!["-p".to_string(), "--output-format".to_string(), "stream-json".to_string()],
            models: vec!["opus".to_string(), "sonnet".to_string(), "haiku".to_string()],
            default_model: "sonnet".to_string(),
            env: HashMap::new(),
            timeout_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub role: String,
    pub ssh_user: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub no_reboot: bool,
    #[serde(default)]
    pub custom_port_required: bool,
}

fn default_ssh_port() -> u16 { 22 }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub protected_services: Vec<String>,
    #[serde(default)]
    pub bypass_permissions: bool,
    #[serde(default = "default_gate_fail_mode")]
    pub gate_fail_mode: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

fn default_gate_fail_mode() -> String {
    "open".to_string()
}

// --- Multi-user auth ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub key: String,
    pub user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub api_keys: Vec<ApiKeyEntry>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig { api_keys: Vec::new() }
    }
}

impl AuthConfig {
    pub fn has_keys(&self) -> bool {
        !self.api_keys.is_empty()
    }

    pub fn first_key(&self) -> Option<&str> {
        self.api_keys.first().map(|e| e.key.as_str())
    }
}

// --- TLS config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

// --- Sessions config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsConfig {
    #[serde(default)]
    pub db_path: Option<String>,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        SessionsConfig { db_path: None }
    }
}

// --- Rate limiting config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rpm")]
    pub requests_per_minute: u32,
    #[serde(default = "default_burst")]
    pub burst: u32,
}

fn default_rpm() -> u32 {
    120
}

fn default_burst() -> u32 {
    20
}

// --- Audit logging config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    #[serde(default)]
    pub db_path: Option<String>,
}

impl Default for AuditConfig {
    fn default() -> Self {
        AuditConfig { db_path: None }
    }
}

// --- Raw auth for TOML parsing (backwards compat) ---

#[derive(Debug, Default, Deserialize)]
struct RawAuthConfig {
    api_key: Option<String>,
    #[serde(default)]
    api_keys: Vec<RawApiKeyEntry>,
}

#[derive(Debug, Deserialize)]
struct RawApiKeyEntry {
    key: String,
    user: String,
}

fn default_home() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".to_string())
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
    pub credd: CreddConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    #[serde(default)]
    pub sessions: SessionsConfig,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub audit: Option<AuditConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let mut agents = HashMap::new();
        agents.insert("claude-code".to_string(), AgentConfig::default());
        Config {
            server: ServerConfig::default(),
            brain: BrainConfig::default(),
            engram: EngramConfig::default(),
            credd: CreddConfig::default(),
            agents,
            servers: Vec::new(),
            safety: SafetyConfig::default(),
            auth: AuthConfig::default(),
            tls: None,
            sessions: SessionsConfig::default(),
            rate_limit: None,
            audit: None,
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
    engram: RawEngramConfig,
    #[serde(default)]
    credd: RawCreddConfig,
    #[serde(default)]
    agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    servers: Vec<ServerEntry>,
    #[serde(default)]
    safety: SafetyConfig,
    #[serde(default)]
    auth: RawAuthConfig,
    #[serde(default)]
    tls: Option<TlsConfig>,
    #[serde(default)]
    sessions: SessionsConfig,
    #[serde(default)]
    rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    audit: Option<AuditConfig>,
}

impl Config {
    pub fn default_path() -> String {
        let home = default_home();
        format!("{}/.config/eidolon/config.toml", home)
    }

    fn build_auth(raw_auth: RawAuthConfig) -> AuthConfig {
        let mut api_keys: Vec<ApiKeyEntry> = Vec::new();

        if let Ok(env_key) = std::env::var("EIDOLON_API_KEY") {
            if !env_key.is_empty() {
                api_keys.push(ApiKeyEntry {
                    key: env_key,
                    user: "default".to_string(),
                });
            }
        }

        for entry in raw_auth.api_keys {
            if api_keys.iter().any(|e| e.key == entry.key) {
                continue;
            }
            api_keys.push(ApiKeyEntry {
                key: entry.key,
                user: entry.user,
            });
        }

        if api_keys.is_empty() {
            if let Some(single_key) = raw_auth.api_key {
                if !single_key.is_empty() {
                    api_keys.push(ApiKeyEntry {
                        key: single_key,
                        user: "default".to_string(),
                    });
                }
            }
        }

        AuthConfig { api_keys }
    }

    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read config {}: {}", path, e))?;

        let raw: RawConfig = toml::from_str(&content)
            .map_err(|e| format!("failed to parse config {}: {}", path, e))?;

        let auth = Self::build_auth(raw.auth);
        let engram_api_key = std::env::var("ENGRAM_API_KEY").ok().or(raw.engram.api_key);
        let credd_url = std::env::var("CREDD_URL").unwrap_or(raw.credd.url);
        let credd_agent_key = std::env::var("CREDD_AGENT_KEY").ok().or(raw.credd.agent_key);

        Ok(Config {
            server: raw.server,
            brain: raw.brain,
            engram: EngramConfig {
                url: raw.engram.url,
                api_key: engram_api_key,
                axon_url: raw.engram.axon_url,
            },
            credd: CreddConfig {
                url: credd_url,
                agent_key: credd_agent_key,
                tier3_trust_threshold: raw.credd.tier3_trust_threshold,
            },
            agents: raw.agents,
            servers: raw.servers,
            safety: raw.safety,
            auth,
            tls: raw.tls,
            sessions: raw.sessions,
            rate_limit: raw.rate_limit,
            audit: raw.audit,
        })
    }

    pub fn load_or_default(path: Option<&str>) -> Result<Self, String> {
        let config_path = path.map(|s| s.to_string()).unwrap_or_else(Self::default_path);

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
            cfg.auth = AuthConfig {
                api_keys: vec![ApiKeyEntry {
                    key: api_key,
                    user: "default".to_string(),
                }],
            };
            cfg.engram.api_key = std::env::var("ENGRAM_API_KEY").ok();
            if let Ok(url) = std::env::var("CREDD_URL") {
                cfg.credd.url = url;
            }
            cfg.credd.agent_key = std::env::var("CREDD_AGENT_KEY").ok();
            cfg
        };

        if config.agents.is_empty() {
            config.agents.insert("claude-code".to_string(), AgentConfig::default());
        }

        Ok(config)
    }

    pub async fn bootstrap_from_credd(&mut self, http: &reqwest::Client) -> Result<(), String> {
        let agent_key = match &self.credd.agent_key {
            Some(k) if !k.is_empty() => k.clone(),
            _ => return Err("credd.agent_key required for bootstrap".to_string()),
        };

        let credd_url = &self.credd.url;

        match credd_fetch(http, credd_url, &agent_key, "engram", "api-key-eidolon").await {
            Ok(secret) => {
                self.engram.api_key = Some(extract_api_key(&secret)?);
                tracing::info!("bootstrapped engram api key from credd");
            }
            Err(e) => {
                if self.engram.api_key.is_none() {
                    return Err(format!("credd: engram api key: {}", e));
                }
                tracing::warn!("credd engram key fetch failed (using config fallback): {}", e);
            }
        }

        let instance_name = if self.server.port == 7700 { "primary" } else { "secondary" };
        match credd_fetch(http, credd_url, &agent_key, "eidolon", instance_name).await {
            Ok(secret) => {
                let key = extract_api_key(&secret)?;
                let existing = self.auth.api_keys.iter_mut().find(|e| e.user == "default");
                if let Some(entry) = existing {
                    entry.key = key;
                } else {
                    self.auth.api_keys.push(ApiKeyEntry {
                        key,
                        user: "default".to_string(),
                    });
                }
                tracing::info!("bootstrapped eidolon api key from credd (instance={})", instance_name);
            }
            Err(e) => {
                if !self.auth.has_keys() {
                    return Err(format!("credd: eidolon api key: {}", e));
                }
                tracing::warn!("credd eidolon key fetch failed (using config fallback): {}", e);
            }
        }

        Ok(())
    }
}

async fn credd_fetch(
    http: &reqwest::Client,
    credd_url: &str,
    agent_key: &str,
    service: &str,
    key: &str,
) -> Result<serde_json::Value, String> {
    let url = format!("{}/secret/{}/{}", credd_url, service, key);
    let resp = http.get(&url)
        .header("Authorization", format!("Bearer {}", agent_key))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("credd fetch {}/{}: {}", service, key, e))?;
    if !resp.status().is_success() {
        return Err(format!("credd {}/{}: HTTP {}", service, key, resp.status()));
    }
    resp.json().await.map_err(|e| format!("credd {}/{} parse: {}", service, key, e))
}

fn extract_api_key(secret: &serde_json::Value) -> Result<String, String> {
    secret.get("value")
        .and_then(|v| v.get("key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "failed to extract key from credd response".to_string())
}
