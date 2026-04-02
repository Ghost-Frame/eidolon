use axum::{
    extract::State,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::config::{Config, ServerEntry};
use crate::secrets::{self, CreddClient};

fn find_server<'a>(host: &str, servers: &'a [ServerEntry]) -> Option<&'a ServerEntry> {
    servers.iter().find(|s| {
        s.name == host || s.aliases.iter().any(|a| a == host)
    })
}

pub async fn gate_check(
    State(state): State<Arc<AppState>>,
    Json(input): Json<Value>,
) -> Json<Value> {
    let tool_name = input.get("tool_name")
        .or_else(|| input.get("tool"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let session_id = input.get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // --- Secret resolution (runs FIRST, before all other gate logic) ---
    let mut modified_input: Option<Value> = None;

    if let Some(ref agent_key) = state.config.credd.agent_key {
        let credd_client = CreddClient::new(
            &state.config.credd.url,
            agent_key,
            state.http_client.clone(),
        );

        let resolution = secrets::resolve_secrets(
            &credd_client,
            &input,
            tool_name,
            session_id,
            state.config.credd.tier3_trust_threshold,
        )
        .await;

        // Log any resolution errors (non-fatal)
        for err in &resolution.errors {
            tracing::warn!("gate: secret resolution error: {} session={}", err, session_id);
        }

        // Track tier-3 values for scrubbing
        if !resolution.tier3_values.is_empty() {
            let mut scrub = state.scrub_registry.lock().await;
            for val in &resolution.tier3_values {
                scrub.track(session_id, val.clone());
            }
        }

        if resolution.modified_input.is_some() {
            tracing::info!("gate: secrets resolved tool={} session={}", tool_name, session_id);
            modified_input = resolution.modified_input;
        }
    }

    // Use modified input for subsequent checks if secrets were resolved
    let effective_input = modified_input.as_ref().unwrap_or(&input);

    let command = effective_input.get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Track Engram store calls for session enforcement
    let is_engram_store =
        (tool_name == "Bash" && (
            command.contains("engram-cli store") ||
            (command.contains("/store") && command.contains(state.config.engram.url.as_str().split("//").last().unwrap_or("")))
        )) ||
        (tool_name.starts_with("mcp__") && tool_name.contains("store"));

    if is_engram_store && session_id != "unknown" {
        let mut sessions = state.sessions.lock().await;
        if let Some(session) = sessions.get_session_mut(session_id, None) {
            session.engram_stores += 1;
            tracing::info!(
                "gate: engram store tracked session={} total={}",
                session_id, session.engram_stores
            );
        }
        sessions.sync_session_to_db(session_id);
    }

    // Fast path: read-only tools always allowed
    match tool_name {
        "Read" | "Glob" | "Grep" | "LS" | "TodoRead" => {
            tracing::debug!("gate: allow (read-only tool) tool={} session={}", tool_name, session_id);
            if let Some(mi) = modified_input {
                return Json(json!({"action": "allow", "modified_input": mi}));
            }
            return Json(json!({"action": "allow"}));
        }
        _ => {}
    }

    // Empty command -- allow
    if command.trim().is_empty() && tool_name != "Bash" {
        if let Some(mi) = modified_input {
            return Json(json!({"action": "allow", "modified_input": mi}));
        }
        return Json(json!({"action": "allow"}));
    }

    // Static dangerous pattern checks (run on resolved command)
    if let Some(block_reason) = check_dangerous_patterns(command, &state.config) {
        tracing::warn!("gate: BLOCK tool={} reason={} session={}", tool_name, block_reason, session_id);
        return Json(json!({
            "action": "block",
            "message": format!("BLOCKED: {}", block_reason),
        }));
    }

    // SSH command checks (block wrong servers, enrich correct ones)
    if command.contains("ssh ") || command.starts_with("ssh") {
        if let Some((mut result, memory_ids)) = check_ssh_command(command, &state).await {
            let action = result.get("action").and_then(|v| v.as_str()).unwrap_or("allow");
            tracing::info!("gate: {} (ssh check) tool={} session={}", action, tool_name, session_id);
            // Attach modified_input if secrets were resolved and action is not block
            if action != "block" {
                if let Some(mi) = modified_input {
                    result.as_object_mut().map(|obj| obj.insert("modified_input".to_string(), mi));
                }
            }

            // F6: Record evolution feedback for brain memories used
            #[cfg(feature = "evolution")]
            if !memory_ids.is_empty() {
                let brain = Arc::clone(&state.brain);
                let useful = result.get("action").and_then(|v| v.as_str()) != Some("block");
                tokio::spawn(async move {
                    let mut brain = brain.lock().await;
                    brain.evolution_feedback(memory_ids, vec![], useful);
                });
            }

            return Json(result);
        }
    }

    // systemctl enrichment
    if command.contains("systemctl ") {
        if let Some((mut enrichment, memory_ids)) = check_systemctl_command(command, &state).await {
            tracing::info!("gate: enrich (systemctl context) tool={} session={}", tool_name, session_id);
            if let Some(mi) = modified_input {
                enrichment.as_object_mut().map(|obj| obj.insert("modified_input".to_string(), mi));
            }

            // F6: Record evolution feedback for brain memories used
            #[cfg(feature = "evolution")]
            if !memory_ids.is_empty() {
                let brain = Arc::clone(&state.brain);
                tokio::spawn(async move {
                    let mut brain = brain.lock().await;
                    brain.evolution_feedback(memory_ids, vec![], true);
                });
            }

            return Json(enrichment);
        }
    }

    // Default: allow (with modified_input if secrets were resolved)
    tracing::debug!("gate: allow tool={} session={}", tool_name, session_id);
    if let Some(mi) = modified_input {
        Json(json!({"action": "allow", "modified_input": mi}))
    } else {
        Json(json!({"action": "allow"}))
    }
}

pub fn check_dangerous_patterns(command: &str, config: &Config) -> Option<String> {
    let cmd_lower = command.to_lowercase();

    // Destructive rm patterns
    if cmd_lower.contains("rm -rf /") && !cmd_lower.contains("rm -rf /tmp") {
        return Some("Destructive rm -rf on critical path -- not allowed".to_string());
    }
    if cmd_lower.contains("rm -rf ~/") {
        return Some("Destructive rm -rf on home directory -- not allowed".to_string());
    }
    if cmd_lower.contains("rm -rf /home") {
        return Some("Destructive rm -rf on /home -- not allowed".to_string());
    }

    // Force push to protected branches
    if cmd_lower.contains("git push") && cmd_lower.contains("--force") {
        if cmd_lower.contains("main") || cmd_lower.contains("master") {
            return Some("Force push to main/master branch blocked".to_string());
        }
    }

    // Hard reset
    if cmd_lower.contains("git reset --hard") {
        return Some("git reset --hard is destructive -- use git stash instead".to_string());
    }

    // Reboot/shutdown: check servers with no_reboot flag
    if cmd_lower.contains("reboot") || cmd_lower.contains("shutdown") {
        for server in &config.servers {
            if server.no_reboot {
                let name_match = cmd_lower.contains(&server.name.to_lowercase());
                let alias_match = server.aliases.iter().any(|a| cmd_lower.contains(&a.to_lowercase()));
                if name_match || alias_match {
                    return Some(format!(
                        "Reboot/shutdown of {} blocked -- {}",
                        server.name, server.notes
                    ));
                }
            }
        }
    }

    // Seed + demo or seed + production -- prevent seeding real data
    if cmd_lower.contains("seed") {
        if cmd_lower.contains("demo") {
            return Some("Seeding demo data blocked -- do not seed demo data into any instance without explicit authorization".to_string());
        }
        if cmd_lower.contains("production") || cmd_lower.contains("prod") {
            return Some("Seeding production data blocked -- do not seed real data into production without explicit authorization".to_string());
        }
    }

    // Stop/restart protected services
    if cmd_lower.contains("systemctl stop") || cmd_lower.contains("systemctl restart")
        || cmd_lower.contains("podman stop") || cmd_lower.contains("docker stop") {
        for svc in &config.safety.protected_services {
            if cmd_lower.contains(&svc.to_lowercase()) {
                return Some(format!(
                    "Stopping/restarting protected service {} requires explicit confirmation",
                    svc
                ));
            }
        }
    }

    // Drop table / format destructors
    if cmd_lower.contains("drop table") {
        return Some("DROP TABLE statement requires manual confirmation".to_string());
    }
    if cmd_lower.contains("mkfs.") {
        return Some("Disk format command blocked -- requires manual confirmation".to_string());
    }

    None
}

#[derive(Debug, Clone)]
pub struct SshTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
}

/// Parse an SSH command string to extract the target host, user, and port.
/// Used for SSRF detection and server map lookups.
pub fn parse_ssh_target(command: &str) -> Option<SshTarget> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let ssh_pos = tokens.iter().position(|&t| t == "ssh")?;

    let mut host_raw: Option<&str> = None;
    let mut port: Option<u16> = None;
    let mut i = ssh_pos + 1;

    while i < tokens.len() {
        let t = tokens[i];
        if t == "-p" || t == "-P" {
            i += 1;
            if i < tokens.len() {
                port = tokens[i].parse::<u16>().ok();
            }
        } else if t.starts_with('-') {
            // Skip flags that take an argument
            if matches!(t, "-i" | "-l" | "-o" | "-L" | "-R" | "-D" | "-J" | "-W") {
                i += 1;
            }
        } else if !t.contains('=') {
            host_raw = Some(t);
            break;
        }
        i += 1;
    }

    let host_raw = host_raw?;
    let (user, host) = if let Some(pos) = host_raw.rfind('@') {
        (Some(host_raw[..pos].to_string()), host_raw[pos + 1..].to_string())
    } else {
        (None, host_raw.to_string())
    };

    Some(SshTarget { user, host, port })
}

async fn check_ssh_command(command: &str, state: &AppState) -> Option<(Value, Vec<i64>)> {
    let target = parse_ssh_target(command)?;
    let host = &target.host;
    let port = target.port;

    // Check against config server map (name or alias match)
    if let Some(server) = find_server(host, &state.config.servers) {
        // If this server requires a custom port and none (or wrong port) is provided
        if server.custom_port_required {
            if port.is_none() || port == Some(22) {
                return Some((json!({
                    "action": "enrich",
                    "message": format!(
                        "Server {} requires a custom SSH port ({}). Notes: {}",
                        server.name, server.ssh_port, server.notes
                    ),
                }), vec![]));
            }
        }
        let notes = if server.no_reboot {
            format!("{} DO NOT REBOOT.", server.notes)
        } else {
            server.notes.clone()
        };
        return Some((json!({
            "action": "enrich",
            "message": format!("Server: {} ({}). SSH user: {}. {}", server.name, server.role, server.ssh_user, notes),
        }), vec![]));
    }

    // Unknown host: query brain for context
    let embed_url = format!("{}/embed", state.config.engram.url);
    let query_text = format!("server {} ssh configuration", host);

    let embed_resp = state.http_client
        .post(&embed_url)
        .json(&json!({"text": query_text}))
        .send()
        .await;

    if let Ok(resp) = embed_resp {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Ok(embedding) = serde_json::from_value::<Vec<f32>>(body["embedding"].clone()) {
                    if !embedding.is_empty() {
                        let mut brain = state.brain.lock().await;
                        let result = brain.query(&embedding, 5, 8.0, 2);
                        let memory_ids: Vec<i64> = result.activated.iter().map(|m| m.id).collect();
                        let top = result.activated.first();
                        if let Some(mem) = top {
                            if mem.activation > 0.5 {
                                return Some((json!({
                                    "action": "enrich",
                                    "message": format!("Brain context for {}: {}", host, mem.content),
                                    "context": mem.content.clone(),
                                }), memory_ids));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

async fn check_systemctl_command(command: &str, state: &AppState) -> Option<(Value, Vec<i64>)> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let systemctl_pos = tokens.iter().position(|&t| t == "systemctl")?;

    let action = tokens.get(systemctl_pos + 1).copied().unwrap_or("");
    let service = tokens.iter()
        .skip(systemctl_pos + 2)
        .find(|&&t| !t.starts_with('-'));

    let service = service.copied()?;

    let embed_url = format!("{}/embed", state.config.engram.url);
    let query_text = format!("systemctl {} {} restart order dependencies", action, service);

    let embed_resp = state.http_client
        .post(&embed_url)
        .json(&json!({"text": query_text}))
        .send()
        .await;

    if let Ok(resp) = embed_resp {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Ok(embedding) = serde_json::from_value::<Vec<f32>>(body["embedding"].clone()) {
                    if !embedding.is_empty() {
                        let mut brain = state.brain.lock().await;
                        let result = brain.query(&embedding, 5, 8.0, 2);
                        let memory_ids: Vec<i64> = result.activated.iter().map(|m| m.id).collect();
                        let top = result.activated.first();
                        if let Some(mem) = top {
                            if mem.activation > 0.5 && mem.content.to_lowercase().contains("restart") {
                                return Some((json!({
                                    "action": "enrich",
                                    "message": format!("Brain context for {} {}: {}", action, service, mem.content),
                                    "context": mem.content.clone(),
                                }), memory_ids));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[derive(serde::Deserialize)]
pub struct CompleteRequest {
    pub session_id: String,
    pub summary: String,
}

pub async fn gate_complete(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CompleteRequest>,
) -> Json<Value> {
    let summary = input.summary.trim().to_string();

    if summary.is_empty() {
        tracing::warn!("gate/complete: blocked -- blank summary session={}", input.session_id);
        return Json(json!({
            "allowed": false,
            "reason": "Summary is required before completing a task -- store what you did"
        }));
    }

    let sessions = state.sessions.lock().await;
    match sessions.get_session(&input.session_id, None) {
        None => {
            tracing::warn!("gate/complete: blocked -- session not found id={}", input.session_id);
            Json(json!({
                "allowed": false,
                "reason": format!("Session '{}' not found -- register with Eidolon before starting work", input.session_id)
            }))
        }
        Some(session) => {
            if session.engram_stores == 0 {
                tracing::warn!(
                    "gate/complete: blocked -- no engram stores session={} agent={}",
                    input.session_id, session.agent
                );
                Json(json!({
                    "allowed": false,
                    "reason": "No Engram stores this session -- store at least one memory before completing"
                }))
            } else {
                tracing::info!(
                    "gate/complete: allowed session={} agent={} stores={}",
                    input.session_id, session.agent, session.engram_stores
                );
                Json(json!({
                    "allowed": true,
                    "reason": format!("{} engram store(s) recorded", session.engram_stores)
                }))
            }
        }
    }
}
