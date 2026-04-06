use std::sync::Arc;
use chrono::Utc;

use crate::AppState;
use crate::session::SessionStatus;
use crate::prompt::generator::generate_prompt;
use crate::absorber::absorb_session;
use super::claude_code::run_claude_code;

pub async fn run_agent(
    state: Arc<AppState>,
    session_id: String,
    agent_name: String,
    task: String,
    model: String,
) {
    // Mark session as Running
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(s) = sessions.get_session_mut(&session_id, None) {
            s.status = SessionStatus::Running;
        }
        sessions.sync_session_to_db(&session_id);
    }

    // Get session user for scoped brain queries
    let session_user = {
        let sessions = state.sessions.lock().await;
        sessions.get_session(&session_id, None)
            .map(|s| s.user.clone())
            .unwrap_or_default()
    };

    // Generate living prompt
    let living_prompt = match generate_prompt(&state, &task, &agent_name, &session_user).await {
        prompt => prompt,
    };

    // Dispatch to agent adapter
    let exit_code = match agent_name.as_str() {
        "claude-code" => {
            match run_claude_code(&state, &session_id, &task, &living_prompt, &model).await {
                Ok(code) => Some(code),
                Err(e) => {
                    tracing::error!("claude-code session {} failed: {}", session_id, e);
                    let mut sessions = state.sessions.lock().await;
                    if let Some(s) = sessions.get_session_mut(&session_id, None) {
                        s.status = SessionStatus::Failed;
                        s.error = Some(e);
                        s.ended_at = Some(Utc::now());
                    }
                    sessions.sync_session_to_db(&session_id);
                    drop(sessions);
                    absorb_session(Arc::clone(&state), session_id).await;
                    return;
                }
            }
        }
        _ => {
            tracing::error!("unknown agent: {}", agent_name);
            let mut sessions = state.sessions.lock().await;
            if let Some(s) = sessions.get_session_mut(&session_id, None) {
                s.status = SessionStatus::Failed;
                s.error = Some(format!("unknown agent: {}", agent_name));
                s.ended_at = Some(Utc::now());
            }
            sessions.sync_session_to_db(&session_id);
            drop(sessions);
            absorb_session(Arc::clone(&state), session_id).await;
            return;
        }
    };

    // Update session with exit result
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(s) = sessions.get_session_mut(&session_id, None) {
            s.exit_code = exit_code;
            s.ended_at = Some(Utc::now());
            if s.status != SessionStatus::Killed && s.status != SessionStatus::TimedOut {
                s.status = match exit_code {
                    Some(0) => SessionStatus::Completed,
                    _ => SessionStatus::Failed,
                };
            }
        }
        sessions.sync_session_to_db(&session_id);
    }

    absorb_session(Arc::clone(&state), session_id).await;
}
