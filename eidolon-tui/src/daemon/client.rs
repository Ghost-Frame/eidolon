use serde_json::{json, Value};
use tokio::sync::mpsc;

pub struct DaemonClient {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct DaemonSession {
    pub session_id: String,
    pub status: String,
}

impl DaemonClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    /// Check if daemon is reachable and authenticated.
    pub async fn health(&self) -> Result<(), String> {
        let url = format!("{}/health", self.base_url);
        let resp = self.http.get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| format!("Daemon unreachable at {}: {}", self.base_url, e))?;

        if !resp.status().is_success() {
            return Err(format!("Daemon health check failed: HTTP {}", resp.status()));
        }
        Ok(())
    }

    /// Submit a task to the daemon. Returns session_id.
    pub async fn submit_task(
        &self,
        task: &str,
        agent: &str,
        model: &str,
    ) -> Result<DaemonSession, String> {
        let url = format!("{}/task", self.base_url);
        let resp = self.http.post(&url)
            .header("Authorization", self.auth_header())
            .json(&json!({
                "task": task,
                "agent": agent,
                "model": model,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to submit task: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Daemon rejected task: {}", body));
        }

        let body: Value = resp.json().await
            .map_err(|e| format!("Failed to parse task response: {}", e))?;

        let session_id = body["session_id"].as_str()
            .ok_or_else(|| "No session_id in response".to_string())?
            .to_string();
        let status = body["status"].as_str().unwrap_or("pending").to_string();

        Ok(DaemonSession { session_id, status })
    }

    /// Stream session output via WebSocket. Sends lines through the mpsc channel.
    /// Returns when the session ends or the connection drops.
    pub async fn stream_session(
        &self,
        session_id: &str,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), String> {
        // Convert HTTP URL to WebSocket URL
        let ws_url = self.base_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let url = format!("{}/task/{}/stream", ws_url, session_id);

        let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| format!("WebSocket connect failed: {}", e))?;

        use futures::StreamExt;
        let (_, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        let msg_type = parsed["type"].as_str().unwrap_or("");
                        match msg_type {
                            "output" => {
                                if let Some(data) = parsed["data"].as_str() {
                                    if tx.send(data.to_string()).is_err() {
                                        break; // Receiver dropped
                                    }
                                }
                            }
                            "session_end" => {
                                let status = parsed["status"].as_str().unwrap_or("unknown");
                                let exit_code = parsed["exit_code"].as_i64().unwrap_or(-1);
                                let _ = tx.send(format!(
                                    "\n[Session ended: {} (exit code {})]",
                                    status, exit_code
                                ));
                                break;
                            }
                            "warning" => {
                                if let Some(msg) = parsed["message"].as_str() {
                                    let _ = tx.send(format!("[warning] {}", msg));
                                }
                            }
                            "error" => {
                                if let Some(msg) = parsed["message"].as_str() {
                                    let _ = tx.send(format!("[error] {}", msg));
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
                Err(e) => {
                    let _ = tx.send(format!("[WebSocket error: {}]", e));
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Gate check via daemon. Returns the daemon's gate response.
    pub async fn gate_check(&self, tool_name: &str, command: &str, session_id: &str) -> Result<Value, String> {
        let url = format!("{}/gate/check", self.base_url);
        let resp = self.http.post(&url)
            .header("Authorization", self.auth_header())
            .json(&json!({
                "tool_name": tool_name,
                "command": command,
                "session_id": session_id,
            }))
            .send()
            .await
            .map_err(|e| format!("Gate check failed: {}", e))?;

        resp.json::<Value>().await
            .map_err(|e| format!("Gate response parse failed: {}", e))
    }

    /// Generate a living prompt for a task via daemon.
    pub async fn generate_prompt(&self, task: &str, agent: &str) -> Result<String, String> {
        let url = format!("{}/prompt/generate", self.base_url);
        let resp = self.http.post(&url)
            .header("Authorization", self.auth_header())
            .json(&json!({
                "task": task,
                "agent": agent,
            }))
            .send()
            .await
            .map_err(|e| format!("Prompt generation failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Daemon prompt generation failed: HTTP {}", resp.status()));
        }

        let body: Value = resp.json().await
            .map_err(|e| format!("Prompt response parse failed: {}", e))?;

        body["prompt"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No prompt in response".to_string())
    }

    /// Get brain stats from daemon.
    pub async fn brain_stats(&self) -> Result<Value, String> {
        let url = format!("{}/brain/stats", self.base_url);
        let resp = self.http.get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| format!("Brain stats failed: {}", e))?;

        resp.json::<Value>().await
            .map_err(|e| format!("Brain stats parse failed: {}", e))
    }

    /// Trigger dream cycle on daemon.
    pub async fn trigger_dream(&self) -> Result<Value, String> {
        let url = format!("{}/brain/dream", self.base_url);
        let resp = self.http.post(&url)
            .header("Authorization", self.auth_header())
            .json(&json!({}))
            .send()
            .await
            .map_err(|e| format!("Dream trigger failed: {}", e))?;

        resp.json::<Value>().await
            .map_err(|e| format!("Dream response parse failed: {}", e))
    }

    /// List all sessions from daemon.
    pub async fn list_sessions(&self) -> Result<Value, String> {
        let url = format!("{}/sessions", self.base_url);
        let resp = self.http.get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| format!("List sessions failed: {}", e))?;

        resp.json::<Value>().await
            .map_err(|e| format!("Sessions response parse failed: {}", e))
    }

    /// Kill a running session.
    pub async fn kill_session(&self, session_id: &str) -> Result<(), String> {
        let url = format!("{}/task/{}/kill", self.base_url, session_id);
        let resp = self.http.post(&url)
            .header("Authorization", self.auth_header())
            .json(&json!({}))
            .send()
            .await
            .map_err(|e| format!("Kill session failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Kill failed: HTTP {}", resp.status()));
        }
        Ok(())
    }
}
