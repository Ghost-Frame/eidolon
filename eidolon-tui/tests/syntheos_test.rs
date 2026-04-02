#[test]
fn test_engram_client_constructs_search_request() {
    use eidolon_tui::syntheos::engram::EngramClient;

    let client = EngramClient::new("http://localhost:4200", "test-key").unwrap();
    let (url, body) = client.build_search_request("test query", 10);
    assert_eq!(url, "http://localhost:4200/search");
    assert!(body.contains("\"query\""));
    assert!(body.contains("test query"));
}

#[test]
fn test_chiasm_client_constructs_create_request() {
    use eidolon_tui::syntheos::chiasm::ChiasmClient;

    let client = ChiasmClient::new("http://localhost:4200", "test-key");
    let (url, body) = client.build_create_task_request("eidolon-tui", "test-project", "Test task");
    assert_eq!(url, "http://localhost:4200/tasks");
    assert!(body.contains("\"agent\""));
    assert!(body.contains("eidolon-tui"));
}

#[test]
fn test_axon_client_constructs_publish_request() {
    use eidolon_tui::syntheos::axon::AxonClient;

    let client = AxonClient::new("http://localhost:4200", "test-key");
    let (url, body) = client.build_publish_request("system", "eidolon-tui", "agent.online", &serde_json::json!({"project": "eidolon"}));
    assert_eq!(url, "http://localhost:4200/axon/publish");
    assert!(body.contains("\"channel\""));
    assert!(body.contains("system"));
}

#[test]
fn test_broca_client_constructs_ask_request() {
    use eidolon_tui::syntheos::broca::BrocaClient;

    let client = BrocaClient::new("http://localhost:4200", "test-key");
    let (url, body) = client.build_ask_request("what deployed recently?");
    assert_eq!(url, "http://localhost:4200/broca/ask");
    assert!(body.contains("\"question\""));
}

#[test]
fn test_openspace_client_constructs_impact_request() {
    use eidolon_tui::syntheos::openspace::OpenSpaceClient;

    let client = OpenSpaceClient::new("http://localhost:4200", "test-key");
    let (url, body) = client.build_impact_request("node-123");
    assert_eq!(url, "http://localhost:4200/structural/impact");
    assert!(body.contains("node-123"));
}
