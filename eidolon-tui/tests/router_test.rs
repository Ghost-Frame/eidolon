#[test]
fn test_parse_routing_response_casual() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let json = r#"{"intent":"casual","confidence":0.95,"tools_needed":[],"agent_needed":null,"reasoning":"General conversation"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    assert_eq!(decision.intent, Intent::Casual);
    assert_eq!(decision.confidence, 0.95);
    assert!(decision.tools_needed.is_empty());
    assert!(decision.agent_needed.is_none());
}

#[test]
fn test_parse_routing_response_memory() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let json = r#"{"intent":"memory","confidence":0.88,"tools_needed":["engram_search","broca_ask"],"agent_needed":null,"reasoning":"Needs infrastructure lookup"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    assert_eq!(decision.intent, Intent::Memory);
    assert_eq!(decision.tools_needed, vec!["engram_search", "broca_ask"]);
}

#[test]
fn test_parse_routing_response_action() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let json = r#"{"intent":"action","confidence":0.92,"tools_needed":["engram_search"],"agent_needed":"claude","reasoning":"Code refactoring task"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    assert_eq!(decision.intent, Intent::Action);
    assert_eq!(decision.agent_needed, Some("claude".to_string()));
}

#[test]
fn test_parse_invalid_json() {
    use eidolon_tui::conversation::router::RoutingDecision;
    let result = RoutingDecision::from_json("not json");
    assert!(result.is_err());
}
