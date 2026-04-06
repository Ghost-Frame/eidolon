use axum::{
    extract::State,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::config::{Config, ServerEntry};
use crate::secrets::{self, CreddClient};

/// Query the brain to validate an action. Returns (action, message, memory_ids).
/// The brain provides context-aware validation beyond static rules.
async fn brain_validate_action(
    state: &AppState,
    command: &str,
    tool_name: &str,
) -> Option<(String, String, Vec<i64>)> {
    // Only validate commands that do something - skip empty/read-only
    if command.trim().is_empty() {
        return None;
    }

    let query_text = format!("{} command: {}", tool_name, command);
    let embedding = state.embed_text(&query_text).await?;

    if embedding.is_empty() {
        return None;
    }

    let mut brain = state.brain.lock().await;
    let result = brain.query(&embedding, 5, 8.0, 2);
    let memory_ids: Vec<i64> = result.activated.iter().map(|m| m.id).collect();

    // Check if any highly-activated memory suggests this action is problematic
    for mem in &result.activated {
        if mem.activation < 0.5 {
            break; // Below confidence threshold
        }
        let content_lower = mem.content.to_lowercase();
        // Brain knows this is a blocked/dangerous pattern
        if (content_lower.contains("blocked") || content_lower.contains("do not")
            || content_lower.contains("never") || content_lower.contains("forbidden")
            || content_lower.contains("not allowed"))
            && command_matches_memory(command, &mem.content)
        {
            return Some((
                "block".to_string(),
                format!("Brain recall (activation {:.2}): {}", mem.activation, mem.content.chars().take(200).collect::<String>()),
                memory_ids,
            ));
        }

        // Brain has enrichment context
        if mem.activation > 0.6 {
            let preview = mem.content.chars().take(300).collect::<String>();
            return Some((
                "enrich".to_string(),
                format!("Brain context: {}", preview),
                memory_ids,
            ));
        }
    }

    // Check contradictions - if the brain resolved a conflict about this topic, share it
    if !result.contradictions.is_empty() {
        let c = &result.contradictions[0];
        if let Some(winner) = result.activated.iter().find(|m| m.id == c.winner_id) {
            return Some((
                "enrich".to_string(),
                format!("Brain resolved conflict: {} (supersedes outdated info)", winner.content.chars().take(200).collect::<String>()),
                memory_ids,
            ));
        }
    }

    None
}

/// Check if a command is related to what a memory is talking about.
/// Simple keyword overlap - the brain's activation already did the heavy lifting.
fn command_matches_memory(command: &str, memory_content: &str) -> bool {
    let cmd_words: std::collections::HashSet<&str> = command
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();
    let mem_words: std::collections::HashSet<&str> = memory_content
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();
    let overlap = cmd_words.intersection(&mem_words).count();
    overlap >= 2
}

fn find_server<'a>(host: &str, servers: &'a [ServerEntry]) -> Option<&'a ServerEntry> {
    servers.iter().find(|s| {
        s.name == host || s.aliases.iter().any(|a| a == host)
    })
}

pub async fn gate_check(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<crate::UserIdentity>,
    Json(input): Json<Value>,
) -> Json<Value> {
    let tool_name = input.get("tool_name")
        .or_else(|| input.get("tool"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let raw_session_id = input.get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let session_id = if raw_session_id != "unknown" {
        match uuid::Uuid::parse_str(raw_session_id) {
            Ok(_) => raw_session_id,
            Err(_) => "unknown",
        }
    } else {
        "unknown"
    };
    let user_filter: Option<&str> = if user.0 == "system" { None } else { Some(user.0.as_str()) };

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

        // Track ALL resolved secret values for scrubbing (not just tier-3)
        if !resolution.resolved_values.is_empty() {
            let mut scrub = state.scrub_registry.lock().await;
            for val in &resolution.resolved_values {
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
    // Require the command to actually invoke engram-cli store (not just mention it in echo/comments)
    let is_engram_store =
        (tool_name == "Bash" && {
            let cmd_trimmed = command.trim_start();
            // Must start with engram-cli or have it after a shell operator, not inside echo/printf
            let has_real_store = cmd_trimmed.starts_with("engram-cli store")
                || command.contains("&& engram-cli store")
                || command.contains("; engram-cli store")
                || command.contains("| engram-cli store");
            let has_curl_store = {
                let engram_host = state.config.engram.url.as_str().split("//").last().unwrap_or("");
                !engram_host.is_empty()
                    && command.contains("/store")
                    && command.contains(engram_host)
                    && (cmd_trimmed.starts_with("curl") || command.contains("&& curl") || command.contains("; curl"))
            };
            has_real_store || has_curl_store
        }) ||
        (tool_name.starts_with("mcp__") && tool_name.contains("store"));

    if is_engram_store && session_id != "unknown" {
        let mut sessions = state.sessions.lock().await;
        if let Some(session) = sessions.get_session_mut(session_id, user_filter) {
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

    // Empty command - allow
    if command.trim().is_empty() && tool_name != "Bash" {
        if let Some(mi) = modified_input {
            return Json(json!({"action": "allow", "modified_input": mi}));
        }
        return Json(json!({"action": "allow"}));
    }

    // Static dangerous pattern checks (run on resolved command)
    if let Some(block_reason) = check_dangerous_patterns(command, &state.config) {
        tracing::warn!("gate: BLOCK tool={} reason={} session={}", tool_name, block_reason, session_id);

        // Increment corrections counter on static blocks too
        if session_id != "unknown" {
            let mut sessions = state.sessions.lock().await;
            if let Some(s) = sessions.get_session_mut(session_id, user_filter) {
                s.corrections += 1;
            }
            sessions.sync_session_to_db(session_id);
        }

        return Json(json!({
            "action": "block",
            "message": format!("BLOCKED: {}", block_reason),
        }));
    }

    // Brain-grounded validation for all non-trivial commands
    if !command.trim().is_empty() && tool_name == "Bash" {
        if let Some((action, message, memory_ids)) = brain_validate_action(&state, command, tool_name).await {
            if action == "block" {
                tracing::warn!("gate: BRAIN BLOCK tool={} session={} reason={}", tool_name, session_id, message);

                // Increment corrections counter
                {
                    let mut sessions = state.sessions.lock().await;
                    if let Some(s) = sessions.get_session_mut(session_id, user_filter) {
                        s.corrections += 1;
                    }
                    sessions.sync_session_to_db(session_id);
                }

                // Evolution feedback: these memories led to a block
                #[cfg(feature = "evolution")]
                if !memory_ids.is_empty() {
                    let brain = Arc::clone(&state.brain);
                    tokio::spawn(async move {
                        let mut brain = brain.lock().await;
                        brain.evolution_feedback(memory_ids, vec![], false);
                    });
                }

                if let Some(mi) = modified_input {
                    return Json(json!({
                        "action": "block",
                        "message": format!("BLOCKED: {}", message),
                        "modified_input": mi,
                    }));
                }
                return Json(json!({
                    "action": "block",
                    "message": format!("BLOCKED: {}", message),
                }));
            }

            if action == "enrich" {
                tracing::info!("gate: BRAIN ENRICH tool={} session={}", tool_name, session_id);

                // Evolution feedback: these memories were useful for enrichment
                #[cfg(feature = "evolution")]
                if !memory_ids.is_empty() {
                    let brain = Arc::clone(&state.brain);
                    tokio::spawn(async move {
                        let mut brain = brain.lock().await;
                        brain.evolution_feedback(memory_ids, vec![], true);
                    });
                }

                let mut result = json!({
                    "action": "enrich",
                    "message": message,
                });
                if let Some(mi) = modified_input {
                    result.as_object_mut().map(|obj| obj.insert("modified_input".to_string(), mi));
                }
                return Json(result);
            }
        }
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

    // Tools requiring human approval via TUI (only when session is known)
    const APPROVAL_TOOLS: &[&str] = &["Bash", "WebFetch", "WebSearch"];
    if session_id != "unknown" && APPROVAL_TOOLS.contains(&tool_name) {
        let approval_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let summary = if !command.is_empty() {
            let truncated: String = command.chars().take(120).collect();
            format!("{}: {}", tool_name, truncated)
        } else {
            let url_or_query = effective_input.get("url")
                .or_else(|| effective_input.get("query"))
                .or_else(|| effective_input.get("prompt"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no details)");
            let truncated: String = url_or_query.chars().take(120).collect();
            format!("{}: {}", tool_name, truncated)
        };

        // Inject permission request into session output
        {
            let mut sessions = state.sessions.lock().await;
            sessions.append_output(session_id, format!("[permission:{}:{}] {}", approval_id, tool_name, summary));
        }

        // Create oneshot channel for approval
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        {
            let mut approvals = state.pending_approvals.lock().await;
            approvals.insert(approval_id.clone(), tx);
        }

        // Wait for approval with timeout
        let approved = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            rx,
        ).await {
            Ok(Ok(decision)) => decision,
            _ => {
                // Timeout or channel closed -- deny
                let mut approvals = state.pending_approvals.lock().await;
                approvals.remove(&approval_id);
                false
            }
        };

        // Clean up
        {
            let mut approvals = state.pending_approvals.lock().await;
            approvals.remove(&approval_id);
        }

        if approved {
            tracing::info!("gate: APPROVED by user tool={} session={}", tool_name, session_id);
            let tool_input = effective_input.clone();
            return Json(json!({"action": "allow", "modified_input": tool_input}));
        } else {
            tracing::warn!("gate: DENIED by user tool={} session={}", tool_name, session_id);
            {
                let mut sessions = state.sessions.lock().await;
                sessions.append_output(session_id, format!("[permission_denied:{}] {} denied by user", approval_id, tool_name));
            }
            return Json(json!({
                "action": "block",
                "message": format!("{} denied by user", tool_name),
            }));
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

#[derive(serde::Deserialize)]
pub struct GateRespondRequest {
    pub approval_id: String,
    pub allow: bool,
}

pub async fn gate_respond(
    State(state): State<Arc<AppState>>,
    Json(input): Json<GateRespondRequest>,
) -> Json<Value> {
    let mut approvals = state.pending_approvals.lock().await;
    if let Some(tx) = approvals.remove(&input.approval_id) {
        let _ = tx.send(input.allow);
        Json(json!({"ok": true}))
    } else {
        Json(json!({"ok": false, "error": "approval not found or expired"}))
    }
}

pub fn check_dangerous_patterns(command: &str, config: &Config) -> Option<String> {
    let cmd_lower = command.to_lowercase();

    // Destructive rm patterns
    if cmd_lower.contains("rm -rf /") && !cmd_lower.contains("rm -rf /tmp") {
        return Some("Destructive rm -rf on critical path - not allowed".to_string());
    }
    if cmd_lower.contains("rm -rf ~/") {
        return Some("Destructive rm -rf on home directory - not allowed".to_string());
    }
    if cmd_lower.contains("rm -rf /home") {
        return Some("Destructive rm -rf on /home - not allowed".to_string());
    }

    // Force push to protected branches
    if cmd_lower.contains("git push") && cmd_lower.contains("--force") {
        if cmd_lower.contains("main") || cmd_lower.contains("master") {
            return Some("Force push to main/master branch blocked".to_string());
        }
    }

    // Hard reset
    if cmd_lower.contains("git reset --hard") {
        return Some("git reset --hard is destructive - use git stash instead".to_string());
    }

    // Reboot/shutdown: check servers with no_reboot flag
    if cmd_lower.contains("reboot") || cmd_lower.contains("shutdown") {
        for server in &config.servers {
            if server.no_reboot {
                let name_match = cmd_lower.contains(&server.name.to_lowercase());
                let alias_match = server.aliases.iter().any(|a| cmd_lower.contains(&a.to_lowercase()));
                if name_match || alias_match {
                    return Some(format!(
                        "Reboot/shutdown of {} blocked - {}",
                        server.name, server.notes
                    ));
                }
            }
        }
    }

    // Seed + demo or seed + production - prevent seeding real data
    if cmd_lower.contains("seed") {
        if cmd_lower.contains("demo") {
            return Some("Seeding demo data blocked - do not seed demo data into any instance without explicit authorization".to_string());
        }
        if cmd_lower.contains("production") || cmd_lower.contains("prod") {
            return Some("Seeding production data blocked - do not seed real data into production without explicit authorization".to_string());
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

    // Secondary interpreter / encoding bypass detection
    // These can be used to smuggle dangerous commands past substring checks
    {
        let tokens: Vec<&str> = cmd_lower.split_whitespace().collect();
        for (i, token) in tokens.iter().enumerate() {
            // python/python3 -c, perl/perl5 -e, ruby -e
            // Also catch full-path invocations like /usr/bin/python3 and env-wrapped calls
            let basename = token.rsplit('/').next().unwrap_or(token);
            let is_interpreter = basename == "python" || basename == "python3"
                || basename.starts_with("python3.")
                || basename == "perl" || basename == "perl5"
                || basename == "ruby";
            // Also catch: env python3 -c
            let is_env_interpreter = *token == "env" && i + 2 < tokens.len() && {
                let next = tokens[i + 1];
                let next_base = next.rsplit('/').next().unwrap_or(next);
                next_base == "python" || next_base == "python3"
                    || next_base.starts_with("python3.")
                    || next_base == "perl" || next_base == "perl5"
                    || next_base == "ruby"
            };
            if is_interpreter {
                if let Some(flag) = tokens.get(i + 1) {
                    if *flag == "-c" || *flag == "-e" {
                        return Some(format!(
                            "Inline code execution via {} {} blocked - use a script file instead",
                            token, flag
                        ));
                    }
                }
            }
            if is_env_interpreter {
                // env python3 -c => flag is at i+2
                if let Some(flag) = tokens.get(i + 2) {
                    if *flag == "-c" || *flag == "-e" {
                        return Some(format!(
                            "Inline code execution via env {} {} blocked - use a script file instead",
                            tokens[i + 1], flag
                        ));
                    }
                }
            }

            // eval with command substitution or string argument
            if *token == "eval" && i + 1 < tokens.len() {
                return Some("eval command blocked - potential command injection vector".to_string());
            }
        }

        // base64 decode piped to sh/bash (base64 -d, base64 --decode, base64 -D)
        let has_base64_decode = cmd_lower.contains("base64 -d")
            || cmd_lower.contains("base64 --decode")
            || cmd_lower.contains("base64 -D"); // macOS variant
        let has_shell_pipe = cmd_lower.contains("| sh")
            || cmd_lower.contains("| bash")
            || cmd_lower.contains("|sh")
            || cmd_lower.contains("|bash")
            || cmd_lower.contains("| /bin/sh")
            || cmd_lower.contains("| /bin/bash");
        if has_base64_decode && has_shell_pipe {
            return Some("base64 decode piped to shell blocked - potential command obfuscation".to_string());
        }

        // xxd -r piped to shell
        if cmd_lower.contains("xxd -r") && has_shell_pipe {
            return Some("hex decode piped to shell blocked - potential command obfuscation".to_string());
        }

        // printf with octal/hex escapes piped to shell
        if cmd_lower.contains("printf") && (cmd_lower.contains("\\x") || cmd_lower.contains("\\0")) && has_shell_pipe {
            return Some("printf escape sequence piped to shell blocked - potential command obfuscation".to_string());
        }
    }

    // Drop table / format destructors
    if cmd_lower.contains("drop table") {
        return Some("DROP TABLE statement requires manual confirmation".to_string());
    }
    if cmd_lower.contains("mkfs.") {
        return Some("Disk format command blocked - requires manual confirmation".to_string());
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

/// Check if an SSH target is a reserved/internal address (SSRF prevention).
/// Parses IPs properly including octal, hex, and decimal-encoded representations.
pub fn is_reserved_ssh_target(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    let host_trimmed = host_lower.trim_matches(|c| c == '[' || c == ']');

    // Try standard IP parse first
    if let Ok(ip) = host_trimmed.parse::<std::net::IpAddr>() {
        return is_ip_reserved(ip);
    }

    // Hostname checks
    if host_trimmed == "localhost"
        || host_trimmed.ends_with(".localhost")
        || host_trimmed == "metadata.google.internal"
        || host_trimmed == "metadata.google"
    {
        return true;
    }

    // Hex-encoded IP: 0x7f000001
    if host_trimmed.starts_with("0x") {
        if let Ok(num) = u32::from_str_radix(&host_trimmed[2..], 16) {
            let ip = std::net::Ipv4Addr::from(num);
            return is_ipv4_reserved(ip);
        }
    }

    // Decimal-encoded IP: 2130706433
    if host_trimmed.chars().all(|c| c.is_ascii_digit()) && !host_trimmed.is_empty() && host_trimmed.len() <= 10 {
        if let Ok(num) = host_trimmed.parse::<u32>() {
            let ip = std::net::Ipv4Addr::from(num);
            return is_ipv4_reserved(ip);
        }
    }

    // Octal-encoded IP: 0177.0.0.1 (leading zeros in octets)
    if host_trimmed.contains('.') {
        let parts: Vec<&str> = host_trimmed.split('.').collect();
        if parts.len() == 4 {
            let has_octal = parts.iter().any(|p| p.starts_with('0') && p.len() > 1 && p.chars().all(|c| c.is_ascii_digit()));
            if has_octal {
                let octets: Option<Vec<u8>> = parts.iter().map(|p| {
                    if p.starts_with('0') && p.len() > 1 && p.chars().all(|c| c.is_ascii_digit()) {
                        u8::from_str_radix(p, 8).ok()
                    } else {
                        p.parse::<u8>().ok()
                    }
                }).collect();
                if let Some(bytes) = octets {
                    let ip = std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
                    return is_ipv4_reserved(ip);
                }
            }
        }
    }

    false
}

fn is_ip_reserved(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_ipv4_reserved(v4),
        std::net::IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() {
                return true;
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_ipv4_reserved(v4);
            }
            // AWS IMDSv2 alternative
            if v6.to_string() == "fd00:ec2::254" {
                return true;
            }
            false
        }
    }
}

fn is_ipv4_reserved(ip: std::net::Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_link_local()
        || ip == std::net::Ipv4Addr::new(169, 254, 169, 254)
}

/// Resolve a hostname and check if ANY of its IPs are reserved/internal.
/// Fails closed: if DNS resolution fails or times out, returns true (block).
async fn resolves_to_reserved(host: &str) -> bool {
    // Skip for raw IPs (already checked by is_reserved_ssh_target)
    if host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }

    let lookup_target = format!("{}:22", host);
    let resolved = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::lookup_host(lookup_target.as_str()),
    ).await;

    match resolved {
        Ok(Ok(addrs)) => {
            for addr in addrs {
                match addr.ip() {
                    std::net::IpAddr::V4(v4) => {
                        if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                            tracing::warn!("gate: SSH DNS rebinding: {} -> {}", host, v4);
                            return true;
                        }
                        if v4.octets()[0] == 169 && v4.octets()[1] == 254 {
                            tracing::warn!("gate: SSH DNS rebinding: {} -> link-local {}", host, v4);
                            return true;
                        }
                    }
                    std::net::IpAddr::V6(v6) => {
                        if v6.is_loopback() || v6.is_unspecified() {
                            tracing::warn!("gate: SSH DNS rebinding: {} -> {}", host, v6);
                            return true;
                        }
                        if let Some(v4) = v6.to_ipv4_mapped() {
                            if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                                tracing::warn!("gate: SSH DNS rebinding: {} -> mapped {}", host, v4);
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Ok(Err(e)) => {
            tracing::warn!("gate: SSH DNS failed for {}: {} (blocking)", host, e);
            true
        }
        Err(_) => {
            tracing::warn!("gate: SSH DNS timeout for {} (blocking)", host);
            true
        }
    }
}

async fn check_ssh_command(command: &str, state: &AppState) -> Option<(Value, Vec<i64>)> {
    let target = parse_ssh_target(command)?;
    let host = &target.host;
    let port = target.port;

    // SSRF prevention: block SSH to reserved/internal targets (hostname check)
    if is_reserved_ssh_target(host) {
        tracing::warn!("gate: SSH SSRF blocked target={}", host);
        return Some((json!({
            "action": "block",
            "message": format!("SSH to reserved/internal target {} blocked (SSRF prevention)", host),
        }), vec![]));
    }

    // DNS rebinding prevention: resolve hostname and check resolved IPs
    if resolves_to_reserved(host).await {
        return Some((json!({
            "action": "block",
            "message": format!("SSH target {} resolves to reserved/internal IP (DNS rebinding prevention)", host),
        }), vec![]));
    }

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
    let query_text = format!("server {} ssh configuration", host);

    if let Some(embedding) = state.embed_text(&query_text).await {
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

    let query_text = format!("systemctl {} {} restart order dependencies", action, service);

    if let Some(embedding) = state.embed_text(&query_text).await {
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

    None
}

#[derive(serde::Deserialize)]
pub struct CompleteRequest {
    pub session_id: String,
    pub summary: String,
}

pub async fn gate_complete(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<crate::UserIdentity>,
    Json(input): Json<CompleteRequest>,
) -> Json<Value> {
    let summary = input.summary.trim().to_string();

    if summary.is_empty() {
        tracing::warn!("gate/complete: blocked - blank summary session={}", input.session_id);
        return Json(json!({
            "allowed": false,
            "reason": "Summary is required before completing a task - store what you did"
        }));
    }

    let user_filter: Option<&str> = if user.0 == "system" { None } else { Some(user.0.as_str()) };
    let sessions = state.sessions.lock().await;
    match sessions.get_session(&input.session_id, user_filter) {
        None => {
            tracing::warn!("gate/complete: blocked - session not found id={}", input.session_id);
            Json(json!({
                "allowed": false,
                "reason": format!("Session '{}' not found - register with Eidolon before starting work", input.session_id)
            }))
        }
        Some(session) => {
            if session.engram_stores == 0 {
                tracing::warn!(
                    "gate/complete: blocked - no engram stores session={} agent={}",
                    input.session_id, session.agent
                );
                Json(json!({
                    "allowed": false,
                    "reason": "No Engram stores this session - store at least one memory before completing"
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
