use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
    TimedOut,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Pending => write!(f, "pending"),
            SessionStatus::Running => write!(f, "running"),
            SessionStatus::Completed => write!(f, "completed"),
            SessionStatus::Failed => write!(f, "failed"),
            SessionStatus::Killed => write!(f, "killed"),
            SessionStatus::TimedOut => write!(f, "timed_out"),
        }
    }
}

pub struct Session {
    pub id: String,
    pub task: String,
    pub agent: String,
    pub model: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub output_buffer: Vec<String>,
    pub output_tx: broadcast::Sender<String>,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    pub corrections: usize,
    pub engram_stores: usize,
    pub error: Option<String>,
}

impl Session {
    pub fn new(task: String, agent: String, model: String) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Session {
            id: Uuid::new_v4().to_string(),
            task,
            agent,
            model,
            status: SessionStatus::Pending,
            created_at: Utc::now(),
            ended_at: None,
            output_buffer: Vec::new(),
            output_tx: tx,
            exit_code: None,
            pid: None,
            corrections: 0,
            engram_stores: 0,
            error: None,
        }
    }

    pub fn append_output(&mut self, line: String) {
        self.output_buffer.push(line.clone());
        // Broadcast -- ignore send errors (no active subscribers is fine)
        let _ = self.output_tx.send(line);
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "task": self.task,
            "agent": self.agent,
            "model": self.model,
            "status": self.status,
            "created_at": self.created_at.to_rfc3339(),
            "ended_at": self.ended_at.map(|t| t.to_rfc3339()),
            "exit_code": self.exit_code,
            "pid": self.pid,
            "corrections": self.corrections,
            "engram_stores": self.engram_stores,
            "error": self.error,
            "output_lines": self.output_buffer.len(),
        })
    }

    pub fn short_id(&self) -> &str {
        &self.id[..8]
    }
}

pub struct SessionManager {
    sessions: HashMap<String, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager {
            sessions: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, task: String, agent: String, model: String) -> String {
        let session = Session::new(task, agent, model);
        let id = session.id.clone();
        self.sessions.insert(id.clone(), session);
        id
    }

    pub fn get_session(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn kill_session(&mut self, id: &str) -> Result<(), String> {
        let session = self.sessions.get_mut(id)
            .ok_or_else(|| format!("session {} not found", id))?;

        if session.status != SessionStatus::Running && session.status != SessionStatus::Pending {
            return Err(format!("session {} is not running (status: {})", id, session.status));
        }

        if let Some(pid) = session.pid {
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(&["/PID", &pid.to_string(), "/F"])
                    .output();
            }
        }

        session.status = SessionStatus::Killed;
        session.ended_at = Some(Utc::now());
        Ok(())
    }

    pub fn list_active(&self) -> Vec<serde_json::Value> {
        self.sessions.values()
            .filter(|s| s.status == SessionStatus::Running || s.status == SessionStatus::Pending)
            .map(|s| s.to_json())
            .collect()
    }

    pub fn list_all(&self) -> Vec<serde_json::Value> {
        let mut all: Vec<_> = self.sessions.values().map(|s| s.to_json()).collect();
        all.sort_by(|a, b| {
            let a_ts = a["created_at"].as_str().unwrap_or("");
            let b_ts = b["created_at"].as_str().unwrap_or("");
            b_ts.cmp(a_ts) // newest first
        });
        all
    }
}
