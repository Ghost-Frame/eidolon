/// Integration tests: spawn the binary, send commands, verify responses.
/// Skips if brain.db is not found (parallel TypeScript pipeline may not have run yet).
/// Uses a synthetic brain.db when BRAIN_DB_PATH env var is not set.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn binary_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target");
    p.push("release");
    p.push("eidolon");
    #[cfg(target_os = "windows")]
    p.set_extension("exe");
    p
}

fn make_synthetic_db() -> tempfile::NamedTempFile {
    use rusqlite::Connection;
    let f = tempfile::NamedTempFile::new().expect("tempfile");
    let conn = Connection::open(f.path()).expect("open temp db");

    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS brain_memories (
            id INTEGER PRIMARY KEY,
            content TEXT NOT NULL,
            category TEXT NOT NULL,
            source TEXT NOT NULL,
            importance INTEGER NOT NULL DEFAULT 5,
            created_at TEXT NOT NULL,
            embedding BLOB,
            tags TEXT,
            curated_at TEXT
        );
        CREATE TABLE IF NOT EXISTS brain_edges (
            source_id INTEGER NOT NULL,
            target_id INTEGER NOT NULL,
            weight REAL NOT NULL DEFAULT 0.5,
            edge_type TEXT NOT NULL DEFAULT 'association',
            created_at TEXT NOT NULL,
            UNIQUE(source_id, target_id)
        );
        CREATE TABLE IF NOT EXISTS brain_meta (
            key TEXT PRIMARY KEY,
            value BLOB,
            updated_at TEXT
        );
    "#).expect("create tables");

    // Insert synthetic memories with 1024-dim embeddings
    let mut stmt = conn.prepare(
        "INSERT INTO brain_memories (id, content, category, source, importance, created_at, embedding) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
    ).expect("prepare");

    for i in 1i64..=10 {
        let content = format!("synthetic memory number {}", i);
        let category = "test";
        let source = "integration-test";
        let importance = 5i32;
        let created_at = format!("2026-03-2{}T00:00:00Z", i);

        // 1024-dim embedding: deterministic values
        let embedding: Vec<f32> = (0..1024).map(|j| {
            ((i as f32 * 0.1 + j as f32 * 0.01) * std::f32::consts::PI).sin()
        }).collect();
        let blob: Vec<u8> = embedding.iter()
            .flat_map(|&f| f.to_le_bytes().to_vec())
            .collect();

        stmt.execute(rusqlite::params![i, content, category, source, importance, created_at, blob])
            .expect("insert memory");
    }

    // Insert a couple of edges
    conn.execute_batch(r#"
        INSERT INTO brain_edges (source_id, target_id, weight, edge_type, created_at)
        VALUES (1, 2, 0.8, 'association', '2026-03-27T00:00:00Z');
        INSERT INTO brain_edges (source_id, target_id, weight, edge_type, created_at)
        VALUES (2, 3, 0.6, 'temporal', '2026-03-27T00:00:00Z');
    "#).expect("insert edges");

    f
}

struct BrainProcess {
    child: std::process::Child,
    reader: BufReader<std::process::ChildStdout>,
    writer: std::process::ChildStdin,
}

impl BrainProcess {
    fn spawn() -> Option<Self> {
        let bin = binary_path();
        if !bin.exists() {
            eprintln!("binary not found at {:?}, skipping integration tests", bin);
            return None;
        }
        let mut child = Command::new(&bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn binary");

        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        let writer = child.stdin.take().expect("stdin");
        Some(BrainProcess { child, reader, writer })
    }

    fn send(&mut self, json: &str) {
        writeln!(self.writer, "{}", json).expect("write to stdin");
        self.writer.flush().expect("flush");
    }

    fn recv(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("read line");
        serde_json::from_str(line.trim()).expect("parse JSON response")
    }

    fn send_recv(&mut self, json: &str) -> serde_json::Value {
        self.send(json);
        self.recv()
    }
}

impl Drop for BrainProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn integration_full_pipeline() {
    let bin = binary_path();
    if !bin.exists() {
        eprintln!("SKIP: binary not found at {:?}", bin);
        return;
    }

    let db_file = make_synthetic_db();
    let db_path = db_file.path().to_str().expect("db path");

    let mut proc = match BrainProcess::spawn() {
        Some(p) => p,
        None => return,
    };

    // Give the process a moment to start
    std::thread::sleep(Duration::from_millis(100));

    // Test get_stats before init (should return empty/error but valid JSON)
    let pre_init = proc.send_recv(r#"{"cmd":"get_stats","seq":0}"#);
    assert!(pre_init.get("ok").is_some(), "response must have ok field");
    assert_eq!(pre_init["cmd"], "get_stats");

    // Init with synthetic db
    let init_cmd = serde_json::json!({
        "cmd": "init",
        "seq": 1,
        "db_path": db_path,
        "data_dir": null
    });
    let init_resp = proc.send_recv(&init_cmd.to_string());
    assert_eq!(init_resp["ok"], true, "init should succeed: {:?}", init_resp);
    assert_eq!(init_resp["cmd"], "init");
    assert_eq!(init_resp["seq"], 1);

    // get_stats
    let stats_resp = proc.send_recv(r#"{"cmd":"get_stats","seq":2}"#);
    assert_eq!(stats_resp["ok"], true, "get_stats should succeed: {:?}", stats_resp);
    assert_eq!(stats_resp["cmd"], "get_stats");
    let data = &stats_resp["data"];
    assert!(data["total_patterns"].as_u64().unwrap_or(0) > 0, "should have patterns");

    // Query with a synthetic embedding
    let embedding: Vec<f32> = (0..1024).map(|i| ((i as f32 * 0.05) * std::f32::consts::PI).sin()).collect();
    let query_cmd = serde_json::json!({
        "cmd": "query",
        "seq": 3,
        "embedding": embedding,
        "top_k": 5,
    });
    let query_resp = proc.send_recv(&query_cmd.to_string());
    assert_eq!(query_resp["ok"], true, "query should succeed: {:?}", query_resp);
    assert_eq!(query_resp["cmd"], "query");
    assert_eq!(query_resp["seq"], 3);
    assert!(query_resp["data"]["activated"].is_array(), "should have activated array");

    // decay_tick
    let decay_resp = proc.send_recv(r#"{"cmd":"decay_tick","seq":4,"ticks":1}"#);
    assert_eq!(decay_resp["ok"], true, "decay_tick should succeed: {:?}", decay_resp);
    assert_eq!(decay_resp["cmd"], "decay_tick");

    // shutdown
    let shutdown_resp = proc.send_recv(r#"{"cmd":"shutdown","seq":5}"#);
    assert_eq!(shutdown_resp["ok"], true, "shutdown should succeed: {:?}", shutdown_resp);
    assert_eq!(shutdown_resp["cmd"], "shutdown");

    // Wait for exit
    let _ = proc.child.wait();
}

#[test]
fn integration_invalid_command() {
    let bin = binary_path();
    if !bin.exists() {
        eprintln!("SKIP: binary not found at {:?}", bin);
        return;
    }

    let mut proc = match BrainProcess::spawn() {
        Some(p) => p,
        None => return,
    };

    std::thread::sleep(Duration::from_millis(100));

    // Send malformed JSON
    let resp = proc.send_recv(r#"{"cmd":"nonexistent_command","seq":99}"#);
    assert_eq!(resp["ok"], false, "invalid command should return ok=false");

    // Send something that's not JSON at all -- will be parse error
    let resp2 = proc.send_recv("this is not json");
    assert_eq!(resp2["ok"], false, "parse error should return ok=false");

    // Now send a valid shutdown
    let _ = proc.send_recv(r#"{"cmd":"shutdown","seq":100}"#);
}
