use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub struct CreddClient {
    http: reqwest::Client,
    base_url: String,
    agent_key: String,
}

pub struct SecretResolution {
    pub modified_input: Option<Value>,
    pub tier3_values: Vec<String>,
    pub errors: Vec<String>,
}

// Tier 1/2: {{secret:svc/key}} or {{secret:svc/key.field}}
fn secret_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\{\{secret:([a-zA-Z0-9_-]+)/([a-zA-Z0-9_-]+)(?:\.([a-zA-Z0-9_]+))?\}\}")
            .unwrap()
    })
}

// Tier 3: {{secret-raw:svc/key.field}}
fn secret_raw_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\{\{secret-raw:([a-zA-Z0-9_-]+)/([a-zA-Z0-9_-]+)\.([a-zA-Z0-9_]+)\}\}")
            .unwrap()
    })
}

impl CreddClient {
    pub fn new(base_url: &str, agent_key: &str, http: reqwest::Client) -> Self {
        CreddClient {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            agent_key: agent_key.to_string(),
        }
    }

    pub async fn fetch(&self, svc: &str, key: &str) -> Result<Value, String> {
        let url = format!("{}/secret/{}/{}", self.base_url, svc, key);
        let resp = self.http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.agent_key))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("credd fetch failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("credd returned {}", resp.status()));
        }

        resp.json::<Value>()
            .await
            .map_err(|e| format!("credd response parse failed: {}", e))
    }

    /// Extract a specific field from a typed secret response.
    /// credd v3 returns: {"type": "ApiKey", "value": {"type": "api_key", "key": "..."}}
    pub fn extract_field(secret: &Value, field: &str) -> Result<String, String> {
        let val = secret.get("value")
            .ok_or_else(|| "secret has no 'value' field".to_string())?;

        val.get(field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("field '{}' not found in secret value", field))
    }

    /// Extract the bare value. Only valid for ApiKey and Note types.
    /// ApiKey: {"type": "ApiKey", "value": {"type": "api_key", "key": "..."}}
    /// Note: {"type": "Note", "value": {"type": "note", "content": "..."}}
    pub fn extract_bare(secret: &Value) -> Result<String, String> {
        let secret_type = secret.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let val = secret.get("value")
            .ok_or_else(|| "secret has no 'value' field".to_string())?;

        match secret_type {
            "ApiKey" => val.get("key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| "ApiKey missing 'key' field".to_string()),
            "Note" => val.get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| "Note missing 'content' field".to_string()),
            other => Err(format!(
                "bare reference not allowed for type '{}' -- use .field syntax",
                other
            )),
        }
    }

    /// For Environment type, build an export block from all key-value pairs.
    pub fn extract_env_export_block(secret: &Value) -> Result<String, String> {
        let secret_type = secret.get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if secret_type != "Environment" {
            return Err(format!("env export block only valid for Environment type, got '{}'", secret_type));
        }

        let val = secret.get("value")
            .and_then(|v| v.as_object())
            .ok_or_else(|| "Environment secret has no value object".to_string())?;

        // Filter out the serde tag field "type" from the export block
        let exports: Vec<String> = val.iter()
            .filter(|(k, _)| k.as_str() != "type")
            .filter_map(|(k, v)| {
                v.as_str().map(|val| {
                    // Shell-escape the value
                    let escaped = val.replace('\'', "'\\''");
                    format!("export {}='{}'", k, escaped)
                })
            })
            .collect();

        if exports.is_empty() {
            return Err("Environment secret has no key-value pairs".to_string());
        }

        Ok(format!("{}; ", exports.join("; ")))
    }
}

/// Resolve all secret placeholders in a JSON value.
/// Returns SecretResolution with the modified input (if any secrets found),
/// tier-3 tracked values, and any errors encountered.
pub async fn resolve_secrets(
    client: &CreddClient,
    input: &Value,
    tool_name: &str,
    session_id: &str,
    trust_threshold: u8,
) -> SecretResolution {
    let mut modified = input.clone();
    let mut had_secrets = false;
    let mut tier3_values = Vec::new();
    let mut errors = Vec::new();

    // Recursively process the JSON tree
    resolve_value(
        client,
        &mut modified,
        tool_name,
        session_id,
        trust_threshold,
        &mut had_secrets,
        &mut tier3_values,
        &mut errors,
    )
    .await;

    SecretResolution {
        modified_input: if had_secrets { Some(modified) } else { None },
        tier3_values,
        errors,
    }
}

/// Trust evaluation seam. Static threshold now, session-decay later.
#[allow(unused_variables)]
pub fn evaluate_trust(session_id: &str) -> u8 {
    // Phase 1: static value
    // Phase 2: track session age, tool call count, gate block count -> decay score
    80
}

/// Recursively walk a JSON value, replacing secret patterns in strings.
async fn resolve_value(
    client: &CreddClient,
    value: &mut Value,
    tool_name: &str,
    session_id: &str,
    trust_threshold: u8,
    had_secrets: &mut bool,
    tier3_values: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    match value {
        Value::String(s) => {
            if let Some(resolved) = resolve_string(
                client,
                s,
                tool_name,
                session_id,
                trust_threshold,
                tier3_values,
                errors,
            )
            .await
            {
                *s = resolved;
                *had_secrets = true;
            }
        }
        Value::Object(map) => {
            // Collect keys first to avoid borrow issues
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(v) = map.get_mut(&key) {
                    Box::pin(resolve_value(
                        client,
                        v,
                        tool_name,
                        session_id,
                        trust_threshold,
                        had_secrets,
                        tier3_values,
                        errors,
                    ))
                    .await;
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                Box::pin(resolve_value(
                    client,
                    item,
                    tool_name,
                    session_id,
                    trust_threshold,
                    had_secrets,
                    tier3_values,
                    errors,
                ))
                .await;
            }
        }
        _ => {}
    }
}

/// Resolve secret patterns in a single string. Returns Some(new_string) if any replacements made.
async fn resolve_string(
    client: &CreddClient,
    input: &str,
    tool_name: &str,
    session_id: &str,
    trust_threshold: u8,
    tier3_values: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> Option<String> {
    let has_tier1 = secret_regex().is_match(input);
    let has_tier3 = secret_raw_regex().is_match(input);

    if !has_tier1 && !has_tier3 {
        return None;
    }

    let mut result = input.to_string();

    // Process Tier 1/2 patterns
    if has_tier1 {
        // Collect all matches first (can't async replace in-place with regex)
        let matches: Vec<(String, String, String, Option<String>)> = secret_regex()
            .captures_iter(input)
            .map(|cap| {
                let full = cap[0].to_string();
                let svc = cap[1].to_string();
                let key = cap[2].to_string();
                let field = cap.get(3).map(|m| m.as_str().to_string());
                (full, svc, key, field)
            })
            .collect();

        for (full_match, svc, key, field) in matches {
            match client.fetch(&svc, &key).await {
                Ok(secret) => {
                    let resolved = if let Some(ref f) = field {
                        CreddClient::extract_field(&secret, f)
                    } else if tool_name == "Bash" {
                        // Tier 2: bare reference in Bash for Environment type
                        let secret_type = secret
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if secret_type == "Environment" {
                            CreddClient::extract_env_export_block(&secret)
                        } else {
                            CreddClient::extract_bare(&secret)
                        }
                    } else {
                        CreddClient::extract_bare(&secret)
                    };

                    match resolved {
                        Ok(val) => {
                            result = result.replace(&full_match, &val);
                        }
                        Err(e) => {
                            errors.push(format!("{}:{}/{}: {}", full_match, svc, key, e));
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("{}: {}", full_match, e));
                }
            }
        }
    }

    // Process Tier 3 patterns
    if has_tier3 {
        let matches: Vec<(String, String, String, String)> = secret_raw_regex()
            .captures_iter(input)
            .map(|cap| {
                let full = cap[0].to_string();
                let svc = cap[1].to_string();
                let key = cap[2].to_string();
                let field = cap[3].to_string();
                (full, svc, key, field)
            })
            .collect();

        let trust = evaluate_trust(session_id);
        if trust < trust_threshold {
            for (full_match, svc, key, _field) in &matches {
                errors.push(format!(
                    "{}: trust score {} below threshold {} for {}/{}",
                    full_match, trust, trust_threshold, svc, key
                ));
            }
        } else {
            for (full_match, svc, key, field) in matches {
                match client.fetch(&svc, &key).await {
                    Ok(secret) => {
                        match CreddClient::extract_field(&secret, &field) {
                            Ok(val) => {
                                tier3_values.push(val.clone());
                                result = result.replace(&full_match, &val);
                            }
                            Err(e) => {
                                errors.push(format!("{}:{}/{}.{}: {}", full_match, svc, key, field, e));
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", full_match, e));
                    }
                }
            }
        }
    }

    if result != input {
        Some(result)
    } else {
        None
    }
}
