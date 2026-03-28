use axum::{
    extract::State,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;

pub async fn gate_check(
    State(state): State<Arc<AppState>>,
    Json(input): Json<Value>,
) -> Json<Value> {
    let tool_name = input.get("tool_name")
        .or_else(|| input.get("tool"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let command = input.get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let session_id = input.get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Fast path: read-only tools always allowed
    match tool_name {
        "Read" | "Glob" | "Grep" | "LS" | "TodoRead" => {
            tracing::debug!("gate: allow (read-only tool) tool={} session={}", tool_name, session_id);
            return Json(json!({"action": "allow"}));
        }
        _ => {}
    }

    // Empty command -- allow
    if command.trim().is_empty() && tool_name != "Bash" {
        return Json(json!({"action": "allow"}));
    }

    // Static dangerous pattern checks
    if let Some(block_reason) = check_dangerous_patterns(command) {
        tracing::warn!("gate: BLOCK tool={} reason={} session={}", tool_name, block_reason, session_id);
        return Json(json!({
            "action": "block",
            "message": format!("BLOCKED: {}", block_reason),
        }));
    }

    // SSH command enrichment
    if command.contains("ssh ") || command.starts_with("ssh") {
        if let Some(enrichment) = check_ssh_command(command, &state).await {
            tracing::info!("gate: enrich (ssh context) tool={} session={}", tool_name, session_id);
            return Json(enrichment);
        }
    }

    // systemctl enrichment
    if command.contains("systemctl ") {
        if let Some(enrichment) = check_systemctl_command(command, &state).await {
            tracing::info!("gate: enrich (systemctl context) tool={} session={}", tool_name, session_id);
            return Json(enrichment);
        }
    }

    // Default: allow
    tracing::debug!("gate: allow tool={} session={}", tool_name, session_id);
    Json(json!({"action": "allow"}))
}

fn check_dangerous_patterns(command: &str) -> Option<&'static str> {
    let cmd_lower = command.to_lowercase();

    // Destructive rm patterns
    if cmd_lower.contains("rm -rf /") && !cmd_lower.contains("rm -rf /tmp") {
        return Some("Destructive rm -rf on critical path -- not allowed");
    }
    if cmd_lower.contains("rm -rf ~/") {
        return Some("Destructive rm -rf on home directory -- not allowed");
    }
    if cmd_lower.contains("rm -rf /home") {
        return Some("Destructive rm -rf on /home -- not allowed");
    }

    // Force push to protected branches
    if cmd_lower.contains("git push") && cmd_lower.contains("--force") {
        if cmd_lower.contains("main") || cmd_lower.contains("master") {
            return Some("Force push to main/master branch blocked");
        }
    }

    // Hard reset
    if cmd_lower.contains("git reset --hard") {
        return Some("git reset --hard is destructive -- use git stash instead");
    }

    // OVH reboot/shutdown (DO NOT REBOOT OVH -- LUKS vault will lock)
    if cmd_lower.contains("reboot") || cmd_lower.contains("shutdown") {
        if cmd_lower.contains("10.0.0.9") || cmd_lower.contains("4822") {
            return Some("Reboot/shutdown of OVH VPS blocked -- LUKS vault will lock");
        }
        // Also block generic reboot on any ssh connection mentioning ovh
        if cmd_lower.contains("ovh") {
            return Some("Reboot/shutdown of OVH VPS blocked -- LUKS vault will lock");
        }
    }

    // Drop table / format destructors
    if cmd_lower.contains("drop table") {
        return Some("DROP TABLE statement requires manual confirmation");
    }
    if cmd_lower.contains("mkfs.") || cmd_lower.contains("format ") {
        return Some("Disk format command blocked -- requires manual confirmation");
    }

    None
}

async fn check_ssh_command(command: &str, state: &AppState) -> Option<Value> {
    // Parse SSH target from command
    // Look for patterns: ssh user@host, ssh -p port user@host, ssh ip
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let ssh_pos = tokens.iter().position(|&t| t == "ssh")?;

    let mut host = None;
    let mut port = None;
    let mut i = ssh_pos + 1;

    while i < tokens.len() {
        let t = tokens[i];
        if t == "-p" || t == "-P" {
            i += 1;
            if i < tokens.len() {
                port = tokens[i].parse::<u16>().ok();
            }
        } else if t.starts_with('-') {
            // Skip other flags (and their args for flags that take args)
            if t == "-i" || t == "-l" || t == "-o" || t == "-L" || t == "-R" || t == "-D" {
                i += 1; // skip the flag's argument
            }
        } else if !t.contains('=') {
            // This is likely the user@host or host token
            host = Some(t);
            break;
        }
        i += 1;
    }

    let host = host?;

    // Check OVH specifically
    if let Some(p) = port {
        if p == 4822 {
            return Some(json!({
                "action": "enrich",
                "message": "OVH VPS connection. DO NOT REBOOT -- LUKS vault will lock. Use port 4822.",
                "context": "OVH VPS: deploy@10.0.0.9 -p 4822. Rootless Podman containers. Do not reboot.",
            }));
        }
    }

    // Query brain for host knowledge
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
    // Extract service name from systemctl command
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let systemctl_pos = tokens.iter().position(|&t| t == "systemctl")?;

    // Format: systemctl [action] [service]
    // action is at systemctl_pos+1, service at systemctl_pos+2 (or with flags)
    let action = tokens.get(systemctl_pos + 1).copied().unwrap_or("");
    let service = tokens.iter()
        .skip(systemctl_pos + 2)
        .find(|&&t| !t.starts_with('-'));

    let service = service.copied()?;

    // Query brain for restart ordering info
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
