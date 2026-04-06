use std::collections::{HashMap, VecDeque};
use std::sync::{Arc as StdArc, Mutex as StdMutex};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_OUTPUT_LINES: usize = 10_000;

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

impl SessionStatus {
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "pending" => SessionStatus::Pending,
            "running" => SessionStatus::Running,
            "completed" => SessionStatus::Completed,
            "failed" => SessionStatus::Failed,
            "killed" => SessionStatus::Killed,
            "timed_out" => SessionStatus::TimedOut,
            _ => SessionStatus::Failed,
        }
    }
}

pub struct Session {
    pub id: String,
    pub task: String,
    pub agent: String,
    pub model: String,
    pub user: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub output_buffer: VecDeque<String>,
    pub output_tx: broadcast::Sender<String>,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    pub corrections: usize,
    pub engram_stores: usize,
    pub error: Option<String>,
}

impl Session {
    pub fn new(task: String, agent: String, model: String, user: String) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Session {
            id: Uuid::new_v4().to_string(),
            task,
            agent,
            model,
            user,
            status: SessionStatus::Pending,
            created_at: Utc::now(),
            ended_at: None,
            output_buffer: VecDeque::new(),
            output_tx: tx,
            exit_code: None,
            pid: None,
            corrections: 0,
            engram_stores: 0,
            error: None,
        }
    }

    pub fn append_output(&mut self, line: String) {
        self.output_buffer.push_back(line.clone());
        if self.output_buffer.len() > MAX_OUTPUT_LINES {
            self.output_buffer.pop_front();
        }
        let _ = self.output_tx.send(line);
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "task": self.task,
            "agent": self.agent,
            "model": self.model,
            "user": self.user,
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
    db: Option<StdArc<StdMutex<Connection>>>,
}

impl SessionManager {
    pub fn new(db_path: Option<&str>) -> Self {
        let db = db_path.and_then(|path| {
            match Connection::open(path) {
                Ok(conn) => {
                    if let Err(e) = Self::init_db(&conn) {
                        tracing::warn!("session db init failed, falling back to in-memory: {}", e);
                        return None;
                    }
                    tracing::info!("session db opened at {}", path);
                    Some(StdArc::new(StdMutex::new(conn)))
                }
                Err(e) => {
                    tracing::warn!("session db open failed ({}), falling back to in-memory: {}", path, e);
                    None
                }
            }
        });

        SessionManager {
            sessions: HashMap::new(),
            db,
        }
    }

    fn init_db(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                task TEXT NOT NULL,
                agent TEXT NOT NULL,
                model TEXT NOT NULL,
                user TEXT NOT NULL DEFAULT 'default',
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                ended_at TEXT,
                exit_code INTEGER,
                pid INTEGER,
                corrections INTEGER DEFAULT 0,
                engram_stores INTEGER DEFAULT 0,
                error TEXT
            );
            CREATE TABLE IF NOT EXISTS session_output (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                line TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_session_output_sid ON session_output(session_id);"
        ).map_err(|e| format!("db init: {}", e))
    }

    pub fn create_session(&mut self, task: String, agent: String, model: String, user: String) -> String {
        let session = Session::new(task, agent, model, user);
        let id = session.id.clone();

        // Insert into DB
        if let Some(ref arc) = self.db {
            let conn = arc.lock().unwrap();
            if let Err(e) = conn.execute(
                "INSERT INTO sessions (id, task, agent, model, user, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    session.id,
                    session.task,
                    session.agent,
                    session.model,
                    session.user,
                    session.status.to_string(),
                    session.created_at.to_rfc3339(),
                ],
            ) {
                tracing::warn!("db insert session failed: {}", e);
            }
        }

        self.sessions.insert(id.clone(), session);
        id
    }

    pub fn append_output(&mut self, session_id: &str, line: String) {
        // Insert into DB
        if let Some(ref arc) = self.db {
            let conn = arc.lock().unwrap();
            let _ = conn.execute(
                "INSERT INTO session_output (session_id, line) VALUES (?1, ?2)",
                params![session_id, &line],
            );
        }

        // Broadcast + buffer in-memory
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.append_output(line);
        }
    }

    /// Get session by id. If user is Some, returns None when user does not match (404 not 403).
    pub fn get_session(&self, id: &str, user: Option<&str>) -> Option<&Session> {
        if let Some(s) = self.sessions.get(id) {
            if let Some(u) = user {
                if s.user != u {
                    return None;
                }
            }
            return Some(s);
        }
        None
    }

    /// Get mutable session by id. If user is Some, returns None when user does not match.
    pub fn get_session_mut(&mut self, id: &str, user: Option<&str>) -> Option<&mut Session> {
        if let Some(s) = self.sessions.get_mut(id) {
            if let Some(u) = user {
                if s.user != u {
                    return None;
                }
            }
            return Some(s);
        }
        None
    }

    /// Sync session fields to DB after in-memory mutation.
    pub fn sync_session_to_db(&self, id: &str) {
        let Some(ref arc) = self.db else { return };
        let Some(s) = self.sessions.get(id) else { return };
        let ended_at = s.ended_at.map(|t| t.to_rfc3339());
        let conn = arc.lock().unwrap();
        let _ = conn.execute(
            "UPDATE sessions SET status = ?1, ended_at = ?2, exit_code = ?3, pid = ?4, corrections = ?5, engram_stores = ?6, error = ?7 WHERE id = ?8",
            params![
                s.status.to_string(),
                ended_at,
                s.exit_code,
                s.pid,
                s.corrections as i64,
                s.engram_stores as i64,
                s.error,
                id,
            ],
        );
    }

    pub fn kill_session(&mut self, id: &str, user: Option<&str>) -> Result<(), String> {
        let session = self.sessions.get_mut(id)
            .ok_or_else(|| format!("session {} not found", id))?;

        if let Some(u) = user {
            if session.user != u {
                return Err(format!("session {} not found", id));
            }
        }

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
        self.sync_session_to_db(id);
        Ok(())
    }

    pub fn list_active(&self, user: &str) -> Vec<serde_json::Value> {
        self.sessions.values()
            .filter(|s| s.user == user)
            .filter(|s| s.status == SessionStatus::Running || s.status == SessionStatus::Pending)
            .map(|s| s.to_json())
            .collect()
    }

    pub fn count_active_global(&self) -> usize {
        self.sessions.values()
            .filter(|s| s.status == SessionStatus::Running || s.status == SessionStatus::Pending)
            .count()
    }

    pub fn list_all(&self, user: &str) -> Vec<serde_json::Value> {
        // In-memory sessions for this user
        let mut all: Vec<_> = self.sessions.values()
            .filter(|s| s.user == user)
            .map(|s| s.to_json())
            .collect();

        // Add historical sessions from DB that are not in memory
        if let Some(ref arc) = self.db {
            let in_memory_ids: std::collections::HashSet<String> = self.sessions.keys().cloned().collect();
            let db_rows: Vec<serde_json::Value> = {
                let conn = arc.lock().unwrap();
                let mut collected = Vec::new();
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT id, task, agent, model, user, status, created_at, ended_at, exit_code, pid, corrections, engram_stores, error FROM sessions WHERE user = ?1 ORDER BY created_at DESC"
                ) {
                    if let Ok(rows) = stmt.query_map(params![user], |row| {
                        let id: String = row.get(0)?;
                        let task: String = row.get(1)?;
                        let agent: String = row.get(2)?;
                        let model: String = row.get(3)?;
                        let user_col: String = row.get(4)?;
                        let status: String = row.get(5)?;
                        let created_at: String = row.get(6)?;
                        let ended_at: Option<String> = row.get(7)?;
                        let exit_code: Option<i32> = row.get(8)?;
                        let pid: Option<u32> = row.get(9)?;
                        let corrections: i64 = row.get(10)?;
                        let engram_stores: i64 = row.get(11)?;
                        let error: Option<String> = row.get(12)?;
                        Ok(serde_json::json!({
                            "id": id,
                            "task": task,
                            "agent": agent,
                            "model": model,
                            "user": user_col,
                            "status": status,
                            "created_at": created_at,
                            "ended_at": ended_at,
                            "exit_code": exit_code,
                            "pid": pid,
                            "corrections": corrections,
                            "engram_stores": engram_stores,
                            "error": error,
                            "output_lines": 0,
                        }))
                    }) {
                        collected = rows.flatten().collect();
                    }
                }
                collected
            };
            for row_result in db_rows {
                let id = row_result["id"].as_str().unwrap_or("").to_string();
                if !in_memory_ids.contains(&id) {
                    all.push(row_result);
                }
            }
        }

        all.sort_by(|a, b| {
            let a_ts = a["created_at"].as_str().unwrap_or("");
            let b_ts = b["created_at"].as_str().unwrap_or("");
            b_ts.cmp(a_ts)
        });
        all
    }

    /// Evict sessions that have reached a terminal status and ended longer ago than max_age.
    pub fn evict_completed(&mut self, max_age: std::time::Duration) {
        let now = Utc::now();
        self.sessions.retain(|_, s| {
            let terminal = matches!(
                s.status,
                SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Killed | SessionStatus::TimedOut
            );
            if !terminal {
                return true;
            }
            match s.ended_at {
                Some(ended) => {
                    let age = now.signed_duration_since(ended);
                    let max_age_chrono = chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::seconds(3600));
                    age < max_age_chrono
                }
                None => true,
            }
        });
    }
}
