use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::Message;

fn daemon_url() -> String {
    std::env::var("EIDOLON_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:7700".to_string())
}

fn api_key() -> String {
    std::env::var("EIDOLON_API_KEY").unwrap_or_default()
}

fn ws_url(base: &str, path: &str) -> String {
    let base = base.replace("http://", "ws://").replace("https://", "wss://");
    format!("{}{}", base, path)
}

fn auth_header() -> Option<(String, String)> {
    let key = api_key();
    if key.is_empty() {
        None
    } else {
        Some(("Authorization".to_string(), format!("Bearer {}", key)))
    }
}

fn client() -> Client {
    let mut builder = Client::builder();
    if let Some((_name, value)) = auth_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            value.parse().unwrap(),
        );
        builder = builder.default_headers(headers);
    }
    builder.build().unwrap()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let result = match args[1].as_str() {
        "health" => cmd_health().await,
        "status" => cmd_status().await,
        "brain" => cmd_brain().await,
        "kill" => {
            if args.len() < 3 {
                eprintln!("usage: eidolon-cli kill <session-id>");
                std::process::exit(1);
            }
            cmd_kill(&args[2]).await
        }
        _ => {
            // Task submission: parse --agent and --model flags
            let mut task_parts: Vec<String> = vec![];
            let mut agent = std::env::var("EIDOLON_AGENT").ok();
            let mut model = std::env::var("EIDOLON_MODEL").ok();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--agent" => {
                        i += 1;
                        if i < args.len() {
                            agent = Some(args[i].clone());
                        }
                    }
                    "--model" => {
                        i += 1;
                        if i < args.len() {
                            model = Some(args[i].clone());
                        }
                    }
                    arg => task_parts.push(arg.to_string()),
                }
                i += 1;
            }
            let task = task_parts.join(" ");
            if task.is_empty() {
                print_usage();
                std::process::exit(1);
            }
            cmd_task(&task, agent.as_deref(), model.as_deref()).await
        }
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!("usage: eidolon-cli [OPTIONS] <task description>");
    eprintln!("       eidolon-cli health");
    eprintln!("       eidolon-cli status");
    eprintln!("       eidolon-cli brain");
    eprintln!("       eidolon-cli kill <session-id>");
    eprintln!("");
    eprintln!("options:");
    eprintln!("  --agent <name>   agent to use (default: claude-code)");
    eprintln!("  --model <name>   model to use (default: agent default)");
    eprintln!("");
    eprintln!("env vars: EIDOLON_URL, EIDOLON_API_KEY, EIDOLON_AGENT, EIDOLON_MODEL");
}

async fn cmd_health() -> Result<(), String> {
    let url = format!("{}/health", daemon_url());
    let resp = client().get(&url).send().await
        .map_err(|e| format!("request failed: {}", e))?;
    let body: Value = resp.json().await
        .map_err(|e| format!("parse failed: {}", e))?;
    println!("{}", serde_json::to_string_pretty(&body).unwrap());
    Ok(())
}

async fn cmd_status() -> Result<(), String> {
    let url = format!("{}/sessions", daemon_url());
    let resp = client().get(&url).send().await
        .map_err(|e| format!("request failed: {}", e))?;

    if resp.status() == 401 {
        return Err("unauthorized - set EIDOLON_API_KEY".to_string());
    }

    let body: Value = resp.json().await
        .map_err(|e| format!("parse failed: {}", e))?;

    let active = body["active"].as_array().cloned().unwrap_or_default();
    if active.is_empty() {
        println!("no active sessions");
    } else {
        println!("{:<38} {:<12} {:<10} {}", "SESSION ID", "STATUS", "AGENT", "TASK");
        println!("{}", "-".repeat(90));
        for s in &active {
            let id = s["id"].as_str().unwrap_or("?");
            let status = s["status"].as_str().unwrap_or("?");
            let agent = s["agent"].as_str().unwrap_or("?");
            let task = s["task"].as_str().unwrap_or("?");
            let task_short: String = task.chars().take(40).collect();
            println!("{:<38} {:<12} {:<10} {}", id, status, agent, task_short);
        }
    }
    Ok(())
}

async fn cmd_brain() -> Result<(), String> {
    let url = format!("{}/brain/stats", daemon_url());
    let resp = client().get(&url).send().await
        .map_err(|e| format!("request failed: {}", e))?;

    if resp.status() == 401 {
        return Err("unauthorized - set EIDOLON_API_KEY".to_string());
    }

    let body: Value = resp.json().await
        .map_err(|e| format!("parse failed: {}", e))?;

    if let Some(stats) = body.get("stats") {
        println!("patterns:    {}", stats["total_patterns"]);
        println!("edges:       {}", stats["total_edges"]);
        println!("avg_act:     {:.4}", stats["avg_activation"].as_f64().unwrap_or(0.0));
        println!("avg_decay:   {:.4}", stats["avg_decay_factor"].as_f64().unwrap_or(0.0));
        if let Some(health) = stats["health_distribution"].as_object() {
            for (k, v) in health {
                println!("health[{}]: {}", k, v);
            }
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&body).unwrap());
    }
    Ok(())
}

async fn cmd_kill(session_id: &str) -> Result<(), String> {
    let url = format!("{}/task/{}/kill", daemon_url(), session_id);
    let resp = client().post(&url).send().await
        .map_err(|e| format!("request failed: {}", e))?;

    if resp.status() == 401 {
        return Err("unauthorized - set EIDOLON_API_KEY".to_string());
    }

    let body: Value = resp.json().await
        .map_err(|e| format!("parse failed: {}", e))?;
    println!("{}", serde_json::to_string_pretty(&body).unwrap());
    Ok(())
}

async fn cmd_task(task: &str, agent: Option<&str>, model: Option<&str>) -> Result<(), String> {
    // Submit task
    let submit_url = format!("{}/task", daemon_url());
    let mut payload = json!({"task": task});
    if let Some(a) = agent {
        payload["agent"] = json!(a);
    }
    if let Some(m) = model {
        payload["model"] = json!(m);
    }

    let resp = client()
        .post(&submit_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("submit failed: {}", e))?;

    if resp.status() == 401 {
        return Err("unauthorized - set EIDOLON_API_KEY".to_string());
    }

    let body: Value = resp.json().await
        .map_err(|e| format!("parse response failed: {}", e))?;

    let session_id = body["session_id"].as_str()
        .ok_or_else(|| format!("unexpected response: {}", body))?;

    eprintln!("session: {}", session_id);

    // Connect WebSocket for streaming
    let ws_url_str = ws_url(&daemon_url(), &format!("/task/{}/stream", session_id));

    let host = daemon_url()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .to_string();

    let mut ws_req_builder = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(&ws_url_str)
        .header("Host", host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key());

    if let Some((_name, val)) = auth_header() {
        ws_req_builder = ws_req_builder.header("Authorization", val);
    }

    let ws_req = ws_req_builder
        .body(())
        .map_err(|e| format!("ws request build failed: {}", e))?;

    let (ws_stream, _) = connect_async(ws_req).await
        .map_err(|e| format!("ws connect failed: {}", e))?;

    let (_, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let v: Value = serde_json::from_str(text.as_str()).unwrap_or(json!({"data": text.as_str()}));
                match v["type"].as_str() {
                    Some("output") => {
                        if let Some(data) = v["data"].as_str() {
                            println!("{}", data);
                        }
                    }
                    Some("session_end") => {
                        let status = v["status"].as_str().unwrap_or("unknown");
                        let exit_code = v["exit_code"].as_i64();
                        eprintln!("session ended: status={} exit_code={:?}", status, exit_code);
                        break;
                    }
                    Some("warning") => {
                        if let Some(msg) = v["message"].as_str() {
                            eprintln!("[warning] {}", msg);
                        }
                    }
                    Some("error") => {
                        if let Some(msg) = v["message"].as_str() {
                            eprintln!("[error] {}", msg);
                        }
                        break;
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                eprintln!("ws error: {}", e);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
