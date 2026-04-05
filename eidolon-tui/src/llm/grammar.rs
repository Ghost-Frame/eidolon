/// GBNF grammar for intent routing. Constrains llama-server output to valid
/// routing JSON with intent classification, confidence, tool list, and agent selection.
pub fn intent_routing_grammar() -> String {
    let s = [
        r#"root ::= "{" ws "\"intent\":" ws intent "," ws "\"confidence\":" ws number "," ws "\"complexity\":" ws complexity "," ws "\"tools_needed\":" ws tools "," ws "\"agent_needed\":" ws agent "," ws "\"reasoning\":" ws string "}" ws"#,
        r#"intent ::= "\"casual\"" | "\"memory\"" | "\"action\"""#,
        r#"complexity ::= "\"light\"" | "\"medium\"" | "\"heavy\"""#,
        r#"agent ::= "\"null\"" | "\"claude\"" | "\"codex\"""#,
        r#"tools ::= "[" ws (string ("," ws string)*)? "]""#,
        r#"number ::= [0-9] "." [0-9] [0-9]?"#,
        r#"string ::= "\"" [a-zA-Z0-9_ .,'!?:;/\-]* "\"" "#,
        "ws ::= [ \\t\\n]*",
    ];
    s.join("\n") + "\n"
}

/// GBNF grammar for context compression output.
/// Constrains to plain text summary (no JSON structure needed).
pub fn compression_grammar() -> String {
    // Compression output is free-form text, but constrain to printable chars
    // and reasonable length to prevent runaway generation.
    let s = [
        r#"root ::= sentence (". " sentence)* "."?"#,
        r#"sentence ::= [A-Za-z0-9_/.:,;'"\-()@#$%&*+=<>!? ]+"#,
    ];
    s.join("\n") + "\n"
}

/// GBNF grammar for prompt distillation output.
/// Constrains to valid JSON with objective, constraints, context, expected_output.
pub fn distillation_grammar() -> String {
    let s = [
        r#"root ::= "{" ws "\"objective\":" ws string "," ws "\"constraints\":" ws constraints "," ws "\"context\":" ws string "," ws "\"expected_output\":" ws string "}" ws"#,
        r#"constraints ::= "[" ws (string ("," ws string)*)? "]""#,
        r#"string ::= "\"" chars "\"" "#,
        r#"chars ::= [a-zA-Z0-9_ .,'!?:;/\-()@#$%&*+=<>{}|~`\[\]\n\\]* "#,
        "ws ::= [ \\t\\n]*",
    ];
    s.join("\n") + "\n"
}

/// GBNF grammar for tool calls. Constrains output to valid tool invocation JSON.
pub fn tool_call_grammar() -> String {
    let s = [
        r#"root ::= "{" ws "\"tool\":" ws tool_name "," ws "\"params\":" ws params "}" ws"#,
        concat!(
            r#"tool_name ::= "\"engram_search\"" | "\"engram_store\"" | "\"engram_recall\"" | "\"engram_context\"""#,
            r#" | "\"chiasm_create\"" | "\"chiasm_update\"" | "\"chiasm_list\"""#,
            r#" | "\"broca_ask\"" | "\"broca_log\"" | "\"broca_feed\"""#,
            r#" | "\"axon_publish\"" | "\"axon_events\"" | "\"soma_heartbeat\"""#,
            r#" | "\"openspace_trace\"" | "\"openspace_impact\"" | "\"openspace_between\"""#,
            r#" | "\"openspace_categorize\"" | "\"openspace_memory_graph\"""#,
        ),
        r#"params ::= "{" ws (param ("," ws param)*)? "}" ws"#,
        r#"param ::= string ":" ws value"#,
        r#"value ::= string | number | "true" | "false" | "null""#,
        r#"number ::= "-"? [0-9]+ ("." [0-9]+)?"#,
        r#"string ::= "\"" [a-zA-Z0-9_ .,'!?:;/@#$%^&*()+={}|<>~`\-]* "\"" "#,
        "ws ::= [ \\t\\n]*",
    ];
    s.join("\n") + "\n"
}

/// GBNF grammar for synthesis output.
/// Constrains to valid JSON with summary, files_changed, key_actions, warnings fields.
pub fn synthesis_grammar() -> String {
    let s = [
        r#"root ::= "{" ws "\"summary\":" ws string "," ws "\"files_changed\":" ws files_changed "," ws "\"key_actions\":" ws key_actions "," ws "\"warnings\":" ws warnings "}" ws"#,
        r#"files_changed ::= "[" ws (string ("," ws string)*)? "]""#,
        r#"key_actions ::= "[" ws (string ("," ws string)*)? "]""#,
        r#"warnings ::= "[" ws (string ("," ws string)*)? "]""#,
        r#"string ::= "\"" chars "\"" "#,
        r#"chars ::= [a-zA-Z0-9_ .,'!?:;/\-()@#$%&*+=<>{}|~`\[\]\n\\]* "#,
        "ws ::= [ \\t\\n]*",
    ];
    s.join("\n") + "\n"
}
