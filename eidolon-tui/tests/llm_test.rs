use eidolon_tui::llm::client::{LlmClient, ChatCompletionRequest, ChatMessage};

#[test]
fn test_build_request_basic() {
    let msgs: &[(&str, &str)] = &[
        ("system", "You are helpful"),
        ("user", "Hello"),
    ];
    let req = LlmClient::build_request(msgs, 0.7, None);
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[1].role, "user");
    assert_eq!(req.temperature, Some(0.7));
    assert!(!req.stream);
    assert!(req.grammar.is_none());
    assert!(req.model.is_none());
}

#[test]
fn test_build_request_with_model() {
    let msgs: &[(&str, &str)] = &[
        ("user", "Hello"),
    ];
    let req = LlmClient::build_request_with_model("qwen3-14b", msgs, 0.5, None);
    assert_eq!(req.model, Some("qwen3-14b".to_string()));
    assert_eq!(req.temperature, Some(0.5));
}

#[test]
fn test_build_request_with_grammar() {
    let msgs: &[(&str, &str)] = &[
        ("user", "Classify this"),
    ];
    let grammar = r#"root ::= "yes" | "no""#;
    let req = LlmClient::build_request(msgs, 0.3, Some(grammar));
    assert_eq!(req.grammar, Some(grammar.to_string()));
}

#[test]
fn test_chat_message_serialization() {
    let msg = ChatMessage {
        role: "user".to_string(),
        content: "Hello world".to_string(),
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "Hello world");
}

#[test]
fn test_request_serialization_skips_none_fields() {
    let req = ChatCompletionRequest {
        model: None,
        messages: vec![ChatMessage { role: "user".to_string(), content: "Hi".to_string() }],
        temperature: Some(0.5),
        max_tokens: None,
        grammar: None,
        stream: false,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("model").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("grammar").is_none());
    assert_eq!(json["stream"], false);
}

#[tokio::test]
async fn test_client_handles_connection_refused() {
    // Connect to a port that's definitely not listening
    let client = LlmClient::new("http://127.0.0.1:19999");
    let msgs: &[(&str, &str)] = &[("user", "test")];
    let req = LlmClient::build_request(msgs, 0.5, None);

    // Should fail with connection error (not panic)
    let result = client.complete(&req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stream_handles_connection_refused() {
    let client = LlmClient::new("http://127.0.0.1:19999");
    let msgs: &[(&str, &str)] = &[("user", "test")];
    let req = LlmClient::build_request(msgs, 0.5, None);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // Should fail gracefully
    let result = client.stream_complete(&req, tx).await;
    assert!(result.is_err());
}
