use axum::{
    extract::{Path, State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;
use crate::UserIdentity;
use crate::session::SessionStatus;

pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<UserIdentity>,
) -> Json<serde_json::Value> {
    let sessions = state.sessions.lock().await;
    Json(json!({
        "ok": true,
        "active": sessions.list_active(&user.0),
        "all": sessions.list_all(&user.0),
    }))
}

pub async fn stream_session(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<UserIdentity>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state, id, user.0))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>, session_id: String, user: String) {
    let (buffered, mut rx, status) = {
        let sessions = state.sessions.lock().await;
        match sessions.get_session(&session_id, Some(&user)) {
            Some(s) => {
                let buf = s.output_buffer.clone();
                let rx = s.output_tx.subscribe();
                let status = s.status.clone();
                (buf, rx, status)
            }
            None => {
                let msg = json!({"type": "error", "message": "session not found"});
                let _ = socket.send(Message::Text(msg.to_string().into())).await;
                return;
            }
        }
    };

    for line in buffered {
        let msg = json!({"type": "output", "data": line});
        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
            return;
        }
    }

    if status != SessionStatus::Pending && status != SessionStatus::Running {
        let sessions = state.sessions.lock().await;
        if let Some(s) = sessions.get_session(&session_id, Some(&user)) {
            let end_msg = json!({
                "type": "session_end",
                "status": s.status,
                "exit_code": s.exit_code,
            });
            let _ = socket.send(Message::Text(end_msg.to_string().into())).await;
        }
        return;
    }

    loop {
        match rx.recv().await {
            Ok(line) => {
                let msg = json!({"type": "output", "data": line});
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                let sessions = state.sessions.lock().await;
                if let Some(s) = sessions.get_session(&session_id, Some(&user)) {
                    let end_msg = json!({
                        "type": "session_end",
                        "status": s.status,
                        "exit_code": s.exit_code,
                    });
                    let _ = socket.send(Message::Text(end_msg.to_string().into())).await;
                }
                return;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                let warn_msg = json!({
                    "type": "warning",
                    "message": format!("lagged: missed {} messages", n),
                });
                if socket.send(Message::Text(warn_msg.to_string().into())).await.is_err() {
                    return;
                }
            }
        }
    }
}
