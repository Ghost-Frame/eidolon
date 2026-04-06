use crate::app::App;
use crate::embedding::AsyncEmbeddingProvider;
use crate::llm::client::LlmClient;
use crate::syntheos::engram::EngramClient;
use crate::conversation::router::RoutingDecision;
use crate::daemon::client::DaemonClient;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Dispatch a user message: fire casual stream immediately + routing in parallel.
/// If routing says memory/action, the casual stream gets aborted and replaced.
pub fn dispatch_message(
    app: &mut App,
    llm_client: &Arc<LlmClient>,
    _engram_client: &Option<Arc<EngramClient>>,
    _embed_provider: &Option<Arc<dyn AsyncEmbeddingProvider>>,
    msg: String,
) {
    app.pending_user_message = msg.clone();

    // Start casual stream immediately -- no waiting
    fire_casual_stream(app, llm_client, &msg);

    // Run router in parallel
    let client = llm_client.clone();
    let model = app.config.llm.model_name.clone();
    let temp = app.config.llm.temperature_routing;
    let msg_clone = msg;
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        let result = RoutingDecision::route(&client, &msg_clone, &model, temp).await;
        let _ = tx.send(result);
    });

    app.routing_rx = Some(rx);
}

/// Fire a casual streaming completion using current conversation history.
pub fn fire_casual_stream(
    app: &mut App,
    llm_client: &Arc<LlmClient>,
    user_msg: &str,
) {
    let history = app.conversation.build_api_messages();
    let (tx, rx) = mpsc::unbounded_channel();
    app.start_streaming(rx);

    let client = llm_client.clone();
    let model = app.config.llm.model_name.clone();
    let temp = app.config.llm.temperature_casual;
    let user_msg = user_msg.to_string();

    let handle = tokio::spawn(async move {
        let mut msgs: Vec<(&str, String)> = Vec::new();
        for (role, content) in &history {
            msgs.push((role.as_str(), content.clone()));
        }
        // Ensure the current user message is included
        if msgs.last().map(|m| m.0) != Some("user") {
            msgs.push(("user", user_msg));
        }

        let msg_refs: Vec<(&str, &str)> = msgs.iter()
            .map(|(r, c)| (*r, c.as_str()))
            .collect();
        let request = LlmClient::build_request_with_model(&model, &msg_refs, temp, None);
        let _ = client.stream_complete(&request, tx).await;
    });
    app.stream_abort = Some(handle.abort_handle());
}

/// Dispatch a task to Claude via the daemon, returning a receiver for output lines.
pub fn dispatch_to_claude(
    task: &str,
    agent: &str,
    model: &str,
    daemon: &Arc<DaemonClient>,
) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let daemon = Arc::clone(daemon);
    let task = task.to_string();
    let agent = agent.to_string();
    let model = model.to_string();

    tokio::spawn(async move {
        // Try to enrich task via daemon prompt generation
        let final_task = match daemon.generate_prompt(&task, &agent).await {
            Ok(enriched) => enriched,
            Err(_) => task,
        };

        match daemon.submit_task(&final_task, &agent, &model).await {
            Ok(session) => {
                let _ = tx.send(format!("[Session {} started]", session.session_id));
                if let Err(e) = daemon.stream_session(&session.session_id, tx.clone()).await {
                    let _ = tx.send(format!("[Stream error: {}]", e));
                }
            }
            Err(e) => {
                let _ = tx.send(format!("[Daemon error: {}]", e));
            }
        }
    });

    rx
}

/// Fire a memory-augmented stream: search Engram for context, inject into conversation.
pub fn fire_memory_stream(
    app: &mut App,
    llm_client: &Arc<LlmClient>,
    engram_client: &Arc<EngramClient>,
    embed_provider: &Option<Arc<dyn AsyncEmbeddingProvider>>,
    user_msg: &str,
) {
    let history = app.conversation.build_api_messages();
    let client = llm_client.clone();
    let engram = engram_client.clone();
    let embed = embed_provider.clone();
    let model = app.config.llm.model_name.clone();
    let temp = app.config.llm.temperature_casual;
    let user_msg = user_msg.to_string();
    let (tx, rx) = mpsc::unbounded_channel();
    app.start_streaming(rx);

    tokio::spawn(async move {
        // Search Engram for relevant context
        let context = match engram.search(&user_msg, 5).await {
            Ok(results) if !results.is_empty() => {
                format!("\n\n[Memory context]\n{}", results.join("\n"))
            }
            _ => {
                // Engram search empty -- try embedding for semantic marker
                if let Some(ref provider) = embed {
                    match provider.embed(&user_msg).await {
                        Ok(vec) => format!("\n\n[Embedded query: {} dims via {}]", vec.len(), provider.name()),
                        Err(_) => String::new(),
                    }
                } else {
                    String::new()
                }
            }
        };

        // Inject context into user message
        let augmented = if context.is_empty() {
            user_msg.clone()
        } else {
            format!("{}{}", user_msg, context)
        };

        let mut msgs: Vec<(&str, String)> = Vec::new();
        for (role, content) in &history {
            msgs.push((role.as_str(), content.clone()));
        }
        // Replace last user message with augmented version
        if let Some(last) = msgs.last_mut() {
            if last.0 == "user" {
                last.1 = augmented;
            }
        }

        let msg_refs: Vec<(&str, &str)> = msgs.iter()
            .map(|(r, c)| (*r, c.as_str()))
            .collect();
        let request = LlmClient::build_request_with_model(&model, &msg_refs, temp, None);
        let _ = client.stream_complete(&request, tx).await;
    });
}
