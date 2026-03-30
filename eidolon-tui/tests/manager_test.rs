#[test]
fn test_new_manager_has_system_prompt() {
    use eidolon_tui::conversation::manager::{ConversationManager, Message, Role};

    let mgr = ConversationManager::new("Test system prompt", 8192, 50);
    let messages = mgr.get_context_window();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, Role::System);
    assert!(messages[0].content.contains("Test system prompt"));
}

#[test]
fn test_add_messages_and_retrieve() {
    use eidolon_tui::conversation::manager::{ConversationManager, Role};

    let mut mgr = ConversationManager::new("System", 8192, 50);
    mgr.add_user_message("Hello");
    mgr.add_assistant_message("Hi there!");
    mgr.add_user_message("How are you?");

    let messages = mgr.get_context_window();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1].role, Role::User);
    assert_eq!(messages[1].content, "Hello");
    assert_eq!(messages[2].role, Role::Assistant);
    assert_eq!(messages[3].role, Role::User);
}

#[test]
fn test_context_window_limits() {
    use eidolon_tui::conversation::manager::{ConversationManager, Role};

    let mut mgr = ConversationManager::new("System", 8192, 5);
    for i in 0..10 {
        mgr.add_user_message(&format!("Message {}", i));
        mgr.add_assistant_message(&format!("Response {}", i));
    }

    let messages = mgr.get_context_window();
    assert!(messages.len() <= 6); // system + 5
    assert_eq!(messages[0].role, Role::System);
}

#[test]
fn test_estimate_tokens() {
    use eidolon_tui::conversation::manager::ConversationManager;

    let mgr = ConversationManager::new("Short system prompt", 8192, 50);
    let tokens = mgr.estimate_context_tokens();
    assert!(tokens > 0);
    assert!(tokens < 100);
}

#[test]
fn test_compaction_produces_summary() {
    use eidolon_tui::conversation::manager::ConversationManager;

    let mut mgr = ConversationManager::new("System", 8192, 5);
    for i in 0..10 {
        mgr.add_user_message(&format!("Tell me about topic {}", i));
        mgr.add_assistant_message(&format!("Here is info about topic {}", i));
    }

    let compacted = mgr.get_compacted_messages();
    assert!(compacted.len() > 0);
    assert_eq!(compacted[0].role, eidolon_tui::conversation::manager::Role::System);
}
