use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::AppState;

fn model_flag(model: &str) -> Option<String> {
    match model {
        "opus" => Some("claude-opus-4-5".to_string()),
        "sonnet" => Some("claude-sonnet-4-5".to_string()),
        "haiku" => Some("claude-haiku-3-5-20241022".to_string()),
        s if s.starts_with("claude-") => Some(s.to_string()),
        _ => None,
    }
}

fn build_gate_hook(daemon_host: &str, daemon_port: u16) -> String {
    // Shell script for Claude Code PreToolUse hook
    // Reads tool input JSON from stdin, POSTs to gate/check, exits 0 (allow) or 2 (block)
    // Fails open if daemon not reachable
    format!(
        r#"#!/bin/bash
INPUT=$(cat)
RESP=$(echo "$INPUT" | curl -sf -X POST http://{host}:{port}/gate/check \
  -H "Content-Type: application/json" \
  -d @- 2>/dev/null)
if [ $? -ne 0 ]; then
  exit 0
fi
ACTION=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('action','allow'))" 2>/dev/null)
if [ "$ACTION" = "block" ]; then
  MSG=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('message','blocked'))" 2>/dev/null)
  echo "$MSG" >&2
  exit 2
fi
# For enrich: print context to stderr (informational only, does not block)
if [ "$ACTION" = "enrich" ]; then
  CTX=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('message',''))" 2>/dev/null)
  echo "[gate] $CTX" >&2
fi
exit 0
"#,
        host = daemon_host,
        port = daemon_port,
    )
}

fn build_settings_json(session_dir: &str, _daemon_host: &str, _daemon_port: u16) -> serde_json::Value {
    let hook_script = format!("{}/gate-hook.sh", session_dir);
    let hook_cmd = format!("bash {} \"$TOOL_NAME\" \"$TOOL_INPUT\"", hook_script);
    serde_json::json!({
        "permissions": {
            "defaultMode": "bypassPermissions"
        },
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": hook_cmd}]
                },
                {
                    "matcher": "Write",
                    "hooks": [{"type": "command", "command": hook_cmd}]
                }
            ]
        }
    })
}

pub async fn run_claude_code(
    state: &Arc<AppState>,
    session_id: &str,
    task: &str,
    living_prompt: &str,
    model: &str,
) -> Result<i32, String> {
    let session_dir = format!("/tmp/eidolon-sessions/{}", session_id);

    // Create session directory structure
    tokio::fs::create_dir_all(&format!("{}/.claude", session_dir))
        .await
        .map_err(|e| format!("failed to create session dir: {}", e))?;

    // Write CLAUDE.md with living prompt
    tokio::fs::write(format!("{}/CLAUDE.md", session_dir), living_prompt)
        .await
        .map_err(|e| format!("failed to write CLAUDE.md: {}", e))?;

    // Write gate hook script
    let daemon_host = &state.config.server.host;
    let daemon_port = state.config.server.port;
    let hook_content = build_gate_hook(daemon_host, daemon_port);
    let hook_path = format!("{}/gate-hook.sh", session_dir);
    tokio::fs::write(&hook_path, &hook_content)
        .await
        .map_err(|e| format!("failed to write gate-hook.sh: {}", e))?;

    // Make hook executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&hook_path).await
            .map_err(|e| format!("failed to stat hook: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&hook_path, perms).await
            .map_err(|e| format!("failed to chmod hook: {}", e))?;
    }

    // Write .claude/settings.json
    let settings = build_settings_json(&session_dir, daemon_host, daemon_port);
    let settings_str = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("failed to serialize settings: {}", e))?;
    tokio::fs::write(format!("{}/.claude/settings.json", session_dir), settings_str)
        .await
        .map_err(|e| format!("failed to write settings.json: {}", e))?;

    // Get agent config
    let agent_cfg = state.config.agents.get("claude-code").cloned();

    // Build command
    let claude_cmd = agent_cfg.as_ref()
        .map(|a| a.command.clone())
        .unwrap_or_else(|| "claude".to_string());

    let mut cmd = Command::new(&claude_cmd);
    cmd.arg("-p").arg(task);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--dangerously-skip-permissions");

    // Add model if specified
    if let Some(model_id) = model_flag(model) {
        cmd.arg("--model").arg(model_id);
    }

    cmd.current_dir(&session_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.env("CLAUDE_CONFIG_DIR", format!("{}/.claude", session_dir));

    // Pass through extra env vars from agent config
    if let Some(ref acfg) = agent_cfg {
        for (k, v) in &acfg.env {
            cmd.env(k, v);
        }
    }

    let mut child = cmd.spawn()
        .map_err(|e| format!("failed to spawn claude: {} -- is 'claude' in PATH?", e))?;

    // Record PID
    let pid = child.id();
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(s) = sessions.get_session_mut(session_id) {
            s.pid = pid;
        }
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Spawn stdout reader
    let state_stdout = Arc::clone(state);
    let sid_stdout = session_id.to_string();
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let mut sessions = state_stdout.sessions.lock().await;
            if let Some(s) = sessions.get_session_mut(&sid_stdout) {
                s.append_output(line);
            }
        }
    });

    // Spawn stderr reader
    let state_stderr = Arc::clone(state);
    let sid_stderr = session_id.to_string();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::debug!("claude stderr [{}]: {}", sid_stderr, line);
            // Also surface stderr lines to session output with prefix
            let mut sessions = state_stderr.sessions.lock().await;
            if let Some(s) = sessions.get_session_mut(&sid_stderr) {
                s.append_output(format!("[stderr] {}", line));
            }
        }
    });

    // Wait for child exit
    let exit_status = child.wait().await
        .map_err(|e| format!("failed to wait for claude: {}", e))?;

    // Join reader tasks
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    // Cleanup temp dir
    let _ = tokio::fs::remove_dir_all(&session_dir).await;

    Ok(exit_status.code().unwrap_or(-1))
}
