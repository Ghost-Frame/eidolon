use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_config_defaults() {
    let config = eidolon_tui::config::Config::default();
    assert_eq!(config.llm.port, 8080);
    assert_eq!(config.llm.context_length, 8192);
    assert_eq!(config.llm.gpu_layers, 99);
    assert_eq!(config.tui.theme, "jujutsu");
    assert_eq!(config.tui.fps, 30);
    assert!(config.tui.animations);
    assert!(config.session.auto_store_to_engram);
}

#[test]
fn test_config_from_toml() {
    let toml_str = r#"
[llm]
model_path = "C:/models/test.gguf"
port = 9090
context_length = 16384

[tui]
theme = "cyberpunk"
fps = 60

[engram]
url = "http://localhost:4200"
api_key = "test-key"
"#;

    let config = eidolon_tui::config::Config::from_str(toml_str).unwrap();
    assert_eq!(config.llm.port, 9090);
    assert_eq!(config.llm.context_length, 16384);
    assert_eq!(config.tui.theme, "cyberpunk");
    assert_eq!(config.tui.fps, 60);
    assert_eq!(config.engram.url, "http://localhost:4200");
}

#[test]
fn test_config_partial_override() {
    let toml_str = r#"
[llm]
model_path = "C:/models/test.gguf"
"#;
    let config = eidolon_tui::config::Config::from_str(toml_str).unwrap();
    assert_eq!(config.llm.port, 8080);
    assert_eq!(config.tui.theme, "jujutsu");
}

#[test]
fn test_config_defaults_have_expected_values() {
    use eidolon_tui::config::Config;
    let config = Config::default();

    assert_eq!(config.llm.model_name, "qwen3-14b");
    assert_eq!(config.llm.context_length, 8192);
    assert_eq!(config.llm.temperature_casual, 0.7);
    assert_eq!(config.llm.temperature_routing, 0.3);
    assert_eq!(config.engram.url, "http://100.64.0.13:4203");
    assert_eq!(config.credd.url, "http://100.64.0.2:4400");
    assert_eq!(config.agents.claude.command, "claude");
    assert_eq!(config.agents.codex.command, "codex");
    assert_eq!(config.session.auto_store_to_engram, true);
    assert_eq!(config.session.max_context_messages, 50);
}

#[test]
fn test_config_agents_have_model_tiers() {
    use eidolon_tui::config::Config;
    let config = Config::default();

    assert!(!config.agents.claude.model_light.is_empty());
    assert!(!config.agents.claude.model_medium.is_empty());
    assert!(!config.agents.claude.model_heavy.is_empty());
    assert!(!config.agents.codex.model_light.is_empty());
    assert!(!config.agents.codex.model_medium.is_empty());
    assert!(!config.agents.codex.model_heavy.is_empty());
}

#[test]
fn test_config_partial_toml_uses_defaults() {
    use eidolon_tui::config::Config;
    let toml = r#"
[llm]
model_name = "custom-model"
"#;
    let config = Config::from_str(toml).unwrap();
    assert_eq!(config.llm.model_name, "custom-model");
    // Other fields should have defaults
    assert_eq!(config.llm.context_length, 8192);
    assert_eq!(config.engram.url, "http://100.64.0.13:4203");
}
