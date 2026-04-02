#[test]
fn test_parse_routing_response_casual() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent, Complexity};
    let json = r#"{"intent":"casual","confidence":0.95,"complexity":"light","tools_needed":[],"agent_needed":null,"reasoning":"General conversation"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    assert_eq!(decision.intent, Intent::Casual);
    assert_eq!(decision.confidence, 0.95);
    assert_eq!(decision.complexity, Complexity::Light);
    assert!(decision.tools_needed.is_empty());
    assert!(decision.agent_needed.is_none());
}

#[test]
fn test_parse_routing_response_memory() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let json = r#"{"intent":"memory","confidence":0.88,"complexity":"medium","tools_needed":["engram_search","broca_ask"],"agent_needed":null,"reasoning":"Needs infrastructure lookup"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    assert_eq!(decision.intent, Intent::Memory);
    assert_eq!(decision.tools_needed, vec!["engram_search", "broca_ask"]);
}

#[test]
fn test_parse_routing_response_action() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let json = r#"{"intent":"action","confidence":0.92,"complexity":"heavy","tools_needed":["engram_search"],"agent_needed":"claude","reasoning":"Code refactoring task"}"#;
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

#[test]
fn test_extract_from_text_with_surrounding_prose() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let text = r#"Here is my analysis: {"intent":"action","confidence":0.80,"complexity":"medium","tools_needed":[],"agent_needed":"claude","reasoning":"Code task"} hope that helps"#;
    let decision = RoutingDecision::extract_from_text(text).unwrap();
    assert_eq!(decision.intent, Intent::Action);
}

#[test]
fn test_keyword_fallback_action() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let decision = RoutingDecision::keyword_fallback("fix the broken authentication module");
    assert_eq!(decision.intent, Intent::Action);
    assert_eq!(decision.agent_needed, Some("claude".to_string()));
}

#[test]
fn test_keyword_fallback_casual() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let decision = RoutingDecision::keyword_fallback("hey how are you doing today");
    assert_eq!(decision.intent, Intent::Casual);
    assert!(decision.agent_needed.is_none());
}

// T2: Extended routing tests

#[test]
fn test_keyword_fallback_memory() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let decision = RoutingDecision::keyword_fallback("recall what we decided about the server");
    assert_eq!(decision.intent, Intent::Memory);
    assert!(decision.agent_needed.is_none());
}

#[test]
fn test_keyword_fallback_confidence_scales() {
    use eidolon_tui::conversation::router::RoutingDecision;

    // Single match -- lower confidence
    let single = RoutingDecision::keyword_fallback("fix something");
    assert!(single.confidence < 0.5, "single match should have low confidence: {}", single.confidence);

    // Multiple matches -- higher confidence
    let multi = RoutingDecision::keyword_fallback("fix and deploy and update the build");
    assert!(multi.confidence > single.confidence, "multi match ({}) should beat single ({})", multi.confidence, single.confidence);
}

#[test]
fn test_keyword_fallback_codex_agent() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let decision = RoutingDecision::keyword_fallback("fix this with codex");
    assert_eq!(decision.intent, Intent::Action);
    assert_eq!(decision.agent_needed, Some("codex".to_string()));
}

#[test]
fn test_extract_from_text_case_insensitive() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    let json = r#"{"intent":"ACTION","confidence":0.8,"complexity":"HEAVY","tools_needed":[],"agent_needed":"claude","reasoning":"test"}"#;
    let decision = RoutingDecision::extract_from_text(json).unwrap();
    assert_eq!(decision.intent, Intent::Action);
}

#[test]
fn test_extract_from_text_keyword_fallback_on_garbage() {
    use eidolon_tui::conversation::router::{RoutingDecision, Intent};
    // No JSON, no keywords -- should be casual
    let decision = RoutingDecision::extract_from_text("hello world how are you").unwrap();
    assert_eq!(decision.intent, Intent::Casual);
}

#[test]
fn test_routing_decision_needs_agent() {
    use eidolon_tui::conversation::router::RoutingDecision;
    let json = r#"{"intent":"action","confidence":0.9,"complexity":"medium","tools_needed":[],"agent_needed":"claude","reasoning":"test"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    assert!(decision.needs_agent());
}

#[test]
fn test_routing_decision_select_model() {
    use eidolon_tui::conversation::router::RoutingDecision;
    use eidolon_tui::config::AgentsConfig;

    let config = AgentsConfig::default();

    // Light complexity
    let json = r#"{"intent":"action","confidence":0.9,"complexity":"light","tools_needed":[],"agent_needed":"claude","reasoning":"test"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    let model = decision.select_model(&config);
    assert_eq!(model, config.claude.model_light);

    // Heavy complexity
    let json = r#"{"intent":"action","confidence":0.9,"complexity":"heavy","tools_needed":[],"agent_needed":"claude","reasoning":"test"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    let model = decision.select_model(&config);
    assert_eq!(model, config.claude.model_heavy);

    // Codex agent
    let json = r#"{"intent":"action","confidence":0.9,"complexity":"medium","tools_needed":[],"agent_needed":"codex","reasoning":"test"}"#;
    let decision = RoutingDecision::from_json(json).unwrap();
    let model = decision.select_model(&config);
    assert_eq!(model, config.codex.model_medium);
}
