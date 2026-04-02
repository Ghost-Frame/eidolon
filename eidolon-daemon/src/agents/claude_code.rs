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

pub fn build_gate_hook(daemon_host: &str, daemon_port: u16, fail_closed: bool) -> String {
    // Shell script for Claude Code PreToolUse hook.
    // Claude Code sends tool input JSON via stdin when it executes the hook command.
    // Exit 0 = allow. Exit 2 = block (stderr message shown to agent). Fails open.
    // When modified_input is present, outputs hookSpecificOutput JSON to stdout
    // so Claude Code uses the modified tool input (with secrets substituted).
    //
    // NOTE: We build the script via string concatenation instead of format!() because
    // the embedded Python code contains dict literals ({...}) that conflict with
    // Rust's format string syntax.
    let mut script = String::from("#!/bin/bash\n");
    script.push_str("# Eidolon gate hook -- called by Claude Code before each tool use.\n");
    script.push_str("INPUT=$(cat)\n");
    script.push_str("if [ -z \"$INPUT\" ]; then\n  exit 0\nfi\n");
    script.push_str(&format!(
        "RESP=$(echo \"$INPUT\" | curl -sf --max-time 3 -X POST http://{}:{}/gate/check \\\n",
        daemon_host, daemon_port
    ));
    script.push_str("  -H \"Content-Type: application/json\" \\\n");
    script.push_str("  -d @- 2>/dev/null)\n");
    script.push_str("CURL_EXIT=$?\n");
    let fail_exit = if fail_closed { "2" } else { "0" };
    script.push_str(&format!("if [ $CURL_EXIT -ne 0 ] || [ -z \"$RESP\" ]; then\n  exit {}\nfi\n", fail_exit));
    // Use Python to parse response. The Python builds hookSpecificOutput JSON
    // when modified_input is present in the gate response.
    script.push_str(r#"ACTION=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('action','allow'))" 2>/dev/null || echo "allow")
MODIFIED_OUTPUT=$(echo "$RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
mi = d.get('modified_input')
if mi is not None:
    out = dict()
    out['hookSpecificOutput'] = dict()
    out['hookSpecificOutput']['hookEventName'] = 'PreToolUse'
    out['hookSpecificOutput']['permissionDecision'] = 'allow'
    out['hookSpecificOutput']['updatedInput'] = mi
    print(json.dumps(out))
" 2>/dev/null)
MSG=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('message',''))" 2>/dev/null)
if [ "$ACTION" = "block" ]; then
  echo "${MSG:-blocked by gate}" >&2
  exit 2
fi
if [ "$ACTION" = "enrich" ] && [ -n "$MSG" ]; then
  echo "[gate] $MSG" >&2
fi
if [ -n "$MODIFIED_OUTPUT" ]; then
  echo "$MODIFIED_OUTPUT"
fi
exit 0
"#);
    script
}

pub fn build_settings_json(hook_path: &str, bypass_permissions: bool) -> serde_json::Value {
    let hook_cmd = format!("bash {}", hook_path);
    let permissions = if bypass_permissions {
        serde_json::json!({
            "defaultMode": "bypassPermissions"
        })
    } else {
        serde_json::json!({
            "defaultMode": "allowEdits",
            "autoApprove": ["Read", "Write", "Glob", "Grep", "Edit", "LS", "TodoRead", "TodoWrite", "NotebookRead"]
        })
    };
    serde_json::json!({
        "permissions": permissions,
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "",
                    "hooks": [{"type": "command", "command": hook_cmd, "timeout": 5}]
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
    let session_dir = {
        let base = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| "/tmp".to_string());
                format!("{}/.local/share/eidolon/sessions", home)
            });
        if base.contains("run/") {
            // XDG_RUNTIME_DIR already has 0700 perms
            format!("{}/eidolon-sessions/{}", base, session_id)
        } else {
            format!("{}/{}", base, session_id)
        }
    };

    // Create session directory structure
    tokio::fs::create_dir_all(&format!("{}/.claude", session_dir))
        .await
        .map_err(|e| format!("failed to create session dir: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_perms = std::fs::Permissions::from_mode(0o700);
        let _ = tokio::fs::set_permissions(&session_dir, dir_perms).await;
    }

    // Write CLAUDE.md with living prompt
    tokio::fs::write(format!("{}/CLAUDE.md", session_dir), living_prompt)
        .await
        .map_err(|e| format!("failed to write CLAUDE.md: {}", e))?;

    // Write gate hook script
    let daemon_host = &state.config.server.host;
    let daemon_port = state.config.server.port;
    let fail_closed = state.config.safety.gate_fail_mode == "closed";
    let hook_content = build_gate_hook(daemon_host, daemon_port, fail_closed);
    let hook_path = format!("{}/gate-hook.sh", session_dir);
    tokio::fs::write(&hook_path, &hook_content)
        .await
        .map_err(|e| format!("failed to write gate-hook.sh: {}", e))?;

    // Make hook executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&hook_path).await
            .map_err(|e| format!("failed to stat hook: {}", e))?
            .permissions();
        perms.set_mode(0o700);
        tokio::fs::set_permissions(&hook_path, perms).await
            .map_err(|e| format!("failed to chmod hook: {}", e))?;
    }

    // Write session settings.json (used via --settings flag)
    let bypass = state.config.safety.bypass_permissions;
    let settings = build_settings_json(&hook_path, bypass);
    let settings_path = format!("{}/.claude/settings.json", session_dir);
    let settings_str = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("failed to serialize settings: {}", e))?;
    tokio::fs::write(&settings_path, settings_str)
        .await
        .map_err(|e| format!("failed to write settings.json: {}", e))?;

    tracing::info!("session={} hook={} settings={}", session_id, hook_path, settings_path);

    // Get agent config
    let agent_cfg = state.config.agents.get("claude-code").cloned();

    // Build command
    let claude_cmd = agent_cfg.as_ref()
        .map(|a| a.command.clone())
        .unwrap_or_else(|| "claude".to_string());

    // Validate binary exists before attempting spawn
    if which::which(&claude_cmd).is_err() {
        return Err(format!(
            "'{}' not found in PATH -- install Claude Code CLI or set agents.claude-code.command in config",
            claude_cmd
        ));
    }

    let mut cmd = Command::new(&claude_cmd);
    cmd.arg("-p").arg(task);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose");

    // Load session settings via --settings flag so hooks actually fire.
    // This merges with the user's global ~/.claude/settings.json.
    cmd.arg("--settings").arg(&settings_path);

    // Add model if specified
    if let Some(model_id) = model_flag(model) {
        cmd.arg("--model").arg(model_id);
    }

    cmd.current_dir(&session_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

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
            let mut sessions = state_stderr.sessions.lock().await;
            if let Some(s) = sessions.get_session_mut(&sid_stderr) {
                s.append_output(format!("[stderr] {}", line));
            }
        }
    });

    // Wait for child exit with timeout
    let timeout_secs = state.config.agents.get("claude-code")
        .and_then(|a| a.timeout_secs)
        .unwrap_or(3600);
    let exit_status = match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        child.wait(),
    ).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => return Err(format!("failed to wait for claude: {}", e)),
        Err(_) => {
            // Timeout -- kill the process
            tracing::warn!("session={} timed out after {}s, killing", session_id, timeout_secs);
            let _ = child.start_kill();
            let _ = child.wait().await;
            // Set session status to TimedOut
            {
                let mut sessions = state.sessions.lock().await;
                if let Some(s) = sessions.get_session_mut(session_id) {
                    s.status = crate::session::SessionStatus::TimedOut;
                    s.ended_at = Some(chrono::Utc::now());
                }
            }
            // Join readers, cleanup, absorb
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            // Absorb session before cleanup
            let absorb_state = Arc::clone(state);
            let absorb_sid = session_id.to_string();
            tokio::spawn(async move {
                crate::absorber::absorb_session(absorb_state, absorb_sid).await;
            });
            let _ = tokio::fs::remove_dir_all(&session_dir).await;
            return Ok(-1);
        }
    };

    // Join reader tasks
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    // Absorb session output to Engram
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(s) = sessions.get_session_mut(session_id) {
            let code = exit_status.code().unwrap_or(-1);
            if code == 0 {
                s.status = crate::session::SessionStatus::Completed;
            } else {
                s.status = crate::session::SessionStatus::Failed;
            }
            s.exit_code = Some(code);
            s.ended_at = Some(chrono::Utc::now());
        }
    }
    let absorb_state = Arc::clone(state);
    let absorb_sid = session_id.to_string();
    tokio::spawn(async move {
        crate::absorber::absorb_session(absorb_state, absorb_sid).await;
    });

    // Cleanup temp dir
    let _ = tokio::fs::remove_dir_all(&session_dir).await;

    Ok(exit_status.code().unwrap_or(-1))
}
