use axum::{
    extract::State,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::secrets::{self, CreddClient};

// Known server mapping: IP/hostname -> (canonical name, notes)
struct ServerInfo {
    canonical: &'static str,
    notes: &'static str,
}

fn server_info(host: &str) -> Option<ServerInfo> {
    match host {
        "10.0.0.1" | "reverse-proxy" => Some(ServerInfo {
            canonical: "reverse-proxy (Hetzner reverse proxy)",
            notes: "This is the Reverse-proxy reverse proxy -- NOT where Engram runs. Engram production is on production (10.0.0.2).",
        }),
        "10.0.0.2" | "production" => Some(ServerInfo {
            canonical: "production (Engram production)",
            notes: "Engram production server. SSH as deploy.",
        }),
        "127.0.0.1" | "10.0.0.3" | "rocky" => Some(ServerInfo {
            canonical: "rocky (staging/backup)",
            notes: "Local staging server. SSH as deploy.",
        }),
        "10.0.0.4" | "10.0.0.4" | "app-server-1" => Some(ServerInfo {
            canonical: "app-server-1",
            notes: "BAV services. SSH as deploy.",
        }),
        "10.0.0.5" | "10.0.0.5" | "edge-server-1" => Some(ServerInfo {
            canonical: "edge-server-1",
            notes: "BAV edge. SSH as deploy.",
        }),
        "10.0.0.6" | "10.0.0.6" | "coolify-host" => Some(ServerInfo {
            canonical: "coolify-host",
            notes: "Coolify server. SSH as root.",
        }),
        "10.0.0.7" | "10.0.0.7" | "app-server-2" => Some(ServerInfo {
            canonical: "app-server-2",
            notes: "Mindset apps. SSH as deploy.",
        }),
        "10.0.0.8" | "10.0.0.8" | "build-server" => Some(ServerInfo {
            canonical: "build-server",
            notes: "Forge. SSH as ghostframe.",
        }),
        "10.0.0.9" | "10.0.0.9" | "container-host" => Some(ServerInfo {
            canonical: "container-host",
            notes: "OVH VPS. Port 4822. DO NOT REBOOT -- LUKS vault will lock.",
        }),
        _ => None,
    }
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
    if let Some(block_reason) = check_dangerous_patterns(command) {
        tracing::warn!("gate: BLOCK tool={} reason={} session={}", tool_name, block_reason, session_id);
        return Json(json!({
            "action": "block",
            "message": format!("BLOCKED: {}", block_reason),
        }));
    }

    // SSH command checks (block wrong servers, enrich correct ones)
    if command.contains("ssh ") || command.starts_with("ssh") {
        if let Some(mut result) = check_ssh_command(command, &state).await {
            let action = result.get("action").and_then(|v| v.as_str()).unwrap_or("allow");
            tracing::info!("gate: {} (ssh check) tool={} session={}", action, tool_name, session_id);
            // Attach modified_input if secrets were resolved and action is not block
            if action != "block" {
                if let Some(mi) = modified_input {
                    result.as_object_mut().map(|obj| obj.insert("modified_input".to_string(), mi));
                }
            }
            return Json(result);
        }
    }

    // systemctl enrichment
    if command.contains("systemctl ") {
        if let Some(mut enrichment) = check_systemctl_command(command, &state).await {
            tracing::info!("gate: enrich (systemctl context) tool={} session={}", tool_name, session_id);
            if let Some(mi) = modified_input {
                enrichment.as_object_mut().map(|obj| obj.insert("modified_input".to_string(), mi));
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

fn check_dangerous_patterns(command: &str) -> Option<String> {
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

    // OVH reboot/shutdown (DO NOT REBOOT OVH -- LUKS vault will lock)
    if cmd_lower.contains("reboot") || cmd_lower.contains("shutdown") {
        if cmd_lower.contains("10.0.0.9") || cmd_lower.contains("4822") {
            return Some("Reboot/shutdown of OVH VPS blocked -- LUKS vault will lock".to_string());
        }
        if cmd_lower.contains("ovh") {
            return Some("Reboot/shutdown of OVH VPS blocked -- LUKS vault will lock".to_string());
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

    // Stop/restart Engram production without confirmation
    if (cmd_lower.contains("systemctl stop") || cmd_lower.contains("systemctl restart")
        || cmd_lower.contains("podman stop") || cmd_lower.contains("docker stop"))
        && cmd_lower.contains("engram")
        && (cmd_lower.contains("10.0.0.2") || cmd_lower.contains("production")) {
        return Some("Stopping/restarting Engram on production (production) requires explicit confirmation from the operator".to_string());
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

async fn check_ssh_command(command: &str, state: &AppState) -> Option<Value> {
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
    // Strip user@ prefix if present
    let host = if let Some(pos) = host_raw.rfind('@') {
        &host_raw[pos + 1..]
    } else {
        host_raw
    };

    // OVH: check port
    if port == Some(4822) || host == "10.0.0.9" {
        if port.is_some() && port != Some(4822) {
            return Some(json!({
                "action": "block",
                "message": "OVH VPS requires port 4822. Use: ssh -i ~/.ssh/id_ed25519 -p 4822 deploy@10.0.0.9. DO NOT REBOOT -- LUKS vault will lock.",
            }));
        }
        return Some(json!({
            "action": "enrich",
            "message": "OVH VPS connection: port 4822, user deploy. DO NOT REBOOT -- LUKS vault will lock. Rootless Podman containers inside. Use SCP + podman cp, not heredoc.",
        }));
    }

    // Reverse-proxy (10.0.0.1) -- check if this is an Engram-related command
    if host == "10.0.0.1" || host == "reverse-proxy" {
        let cmd_lower = command.to_lowercase();
        if cmd_lower.contains("engram") || cmd_lower.contains("4200") || cmd_lower.contains("brain") {
            return Some(json!({
                "action": "block",
                "message": "WRONG SERVER: Engram production runs on production (10.0.0.2), NOT reverse-proxy (10.0.0.1). Reverse-proxy is the reverse proxy only. Use: ssh -i ~/.ssh/id_ed25519 deploy@10.0.0.2",
            }));
        }
        return Some(json!({
            "action": "enrich",
            "message": "Note: reverse-proxy (10.0.0.1) is the reverse proxy. If you need Engram, use production (10.0.0.2) instead.",
        }));
    }

    // Check against known server map
    if let Some(info) = server_info(host) {
        return Some(json!({
            "action": "enrich",
            "message": format!("Server: {} -- {}", info.canonical, info.notes),
        }));
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
                        let top = result.activated.first();
                        if let Some(mem) = top {
                            if mem.activation > 0.5 {
                                return Some(json!({
                                    "action": "enrich",
                                    "message": format!("Brain context for {}: {}", host, mem.content),
                                    "context": mem.content.clone(),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

async fn check_systemctl_command(command: &str, state: &AppState) -> Option<Value> {
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
                        let top = result.activated.first();
                        if let Some(mem) = top {
                            if mem.activation > 0.5 && mem.content.to_lowercase().contains("restart") {
                                return Some(json!({
                                    "action": "enrich",
                                    "message": format!("Brain context for {} {}: {}", action, service, mem.content),
                                    "context": mem.content.clone(),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
