#[test]
fn test_intent_grammar_is_valid_gbnf() {
    let grammar = eidolon_tui::llm::grammar::intent_routing_grammar();
    assert!(!grammar.is_empty());
    assert!(grammar.contains("root ::="));
    assert!(grammar.contains("intent"));
    assert!(grammar.contains("\\\"casual\\\""));
    assert!(grammar.contains("\\\"memory\\\""));
    assert!(grammar.contains("\\\"action\\\""));
}

#[test]
fn test_tool_call_grammar_is_valid_gbnf() {
    let grammar = eidolon_tui::llm::grammar::tool_call_grammar();
    assert!(!grammar.is_empty());
    assert!(grammar.contains("root ::="));
    assert!(grammar.contains("tool_name"));
}
