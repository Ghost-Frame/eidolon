use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: String,
    pub user: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub source_ip: String,
    pub user_agent: String,
}

pub struct AuditRecord {
    pub timestamp: String,
    pub user: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub source_ip: String,
    pub user_agent: String,
}

pub struct AuditLog {
    writer_tx: mpsc::UnboundedSender<AuditRecord>,
    reader: std::sync::Arc<std::sync::Mutex<Connection>>,
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            user TEXT NOT NULL,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            status_code INTEGER NOT NULL DEFAULT 0,
            source_ip TEXT NOT NULL DEFAULT '',
            user_agent TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_log(user);
        CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);",
    )
    .map_err(|e| format!("failed to init audit schema: {}", e))
}

impl AuditLog {
    pub fn open(db_path: &str) -> Result<Self, String> {
        // Writer connection (owned by background thread)
        let write_conn = Connection::open(db_path)
            .map_err(|e| format!("failed to open audit db {}: {}", db_path, e))?;
        write_conn
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("failed to set audit pragmas: {}", e))?;
        init_schema(&write_conn)?;

        // Reader connection (for queries)
        let read_conn = Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("failed to open audit reader: {}", e))?;

        let reader = std::sync::Arc::new(std::sync::Mutex::new(read_conn));

        let (tx, mut rx) = mpsc::unbounded_channel::<AuditRecord>();

        // Background writer thread (std::thread because Connection is !Send in tokio context)
        std::thread::spawn(move || {
            while let Some(rec) = rx.blocking_recv() {
                let result = write_conn.execute(
                    "INSERT INTO audit_log (timestamp, user, method, path, status_code, source_ip, user_agent)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        rec.timestamp,
                        rec.user,
                        rec.method,
                        rec.path,
                        rec.status_code,
                        rec.source_ip,
                        rec.user_agent,
                    ],
                );
                if let Err(e) = result {
                    tracing::warn!("audit write failed: {}", e);
                }
            }
        });

        Ok(AuditLog {
            writer_tx: tx,
            reader,
        })
    }

    /// Fire-and-forget audit record. Never blocks, never fails visibly.
    pub fn record(&self, rec: AuditRecord) {
        let _ = self.writer_tx.send(rec);
    }

    /// Query audit entries for a specific user. Runs on a blocking thread.
    pub async fn query(
        &self,
        user: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditEntry>, String> {
        let reader = self.reader.clone();
        let user = user.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = reader
                .lock()
                .map_err(|e| format!("audit reader lock: {}", e))?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, timestamp, user, method, path, status_code, source_ip, user_agent
                     FROM audit_log
                     WHERE user = ?1
                     ORDER BY id DESC
                     LIMIT ?2 OFFSET ?3",
                )
                .map_err(|e| format!("audit query prepare: {}", e))?;

            let rows = stmt
                .query_map(rusqlite::params![user, limit, offset], |row| {
                    Ok(AuditEntry {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        user: row.get(2)?,
                        method: row.get(3)?,
                        path: row.get(4)?,
                        status_code: row.get::<_, i32>(5)? as u16,
                        source_ip: row.get(6)?,
                        user_agent: row.get(7)?,
                    })
                })
                .map_err(|e| format!("audit query: {}", e))?;

            let mut entries = Vec::new();
            for row in rows {
                match row {
                    Ok(entry) => entries.push(entry),
                    Err(e) => tracing::warn!("audit row error: {}", e),
                }
            }
            Ok(entries)
        })
        .await
        .map_err(|e| format!("audit query task: {}", e))?
    }
}
