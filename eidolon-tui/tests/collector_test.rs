use tempfile::TempDir;

#[test]
fn test_collector_writes_jsonl() {
    use eidolon_tui::dataset::collector::{DatasetCollector, TrainingExample};

    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("training.jsonl");
    let mut collector = DatasetCollector::new(path.clone());

    let example = TrainingExample {
        system_prompt: "You are Gojo.".to_string(),
        user_message: "What's the weather?".to_string(),
        assistant_response: "Hmm, that's a memory question.".to_string(),
        intent: "memory".to_string(),
        tools_called: vec!["engram_search".to_string()],
        user_override: false,
    };

    collector.record(example).unwrap();
    collector.flush().unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("\"intent\":\"memory\""));
    assert!(contents.contains("You are Gojo."));
}

#[test]
fn test_collector_appends_multiple() {
    use eidolon_tui::dataset::collector::{DatasetCollector, TrainingExample};

    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("training.jsonl");
    let mut collector = DatasetCollector::new(path.clone());

    for i in 0..3 {
        collector.record(TrainingExample {
            system_prompt: "System".to_string(),
            user_message: format!("Message {}", i),
            assistant_response: format!("Response {}", i),
            intent: "casual".to_string(),
            tools_called: vec![],
            user_override: false,
        }).unwrap();
    }
    collector.flush().unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 3);
}
