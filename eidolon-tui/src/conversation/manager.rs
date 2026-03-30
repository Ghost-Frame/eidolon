use chrono::Utc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub timestamp: chrono::DateTime<Utc>,
    pub pinned: bool,
}

pub struct ConversationManager {
    messages: Vec<Message>,
    system_prompt: String,
    max_context_tokens: u32,
    max_context_messages: u32,
}

impl ConversationManager {
    pub fn new(system_prompt: &str, max_context_tokens: u32, max_context_messages: u32) -> Self {
        let system_msg = Message {
            role: Role::System,
            content: system_prompt.to_string(),
            timestamp: Utc::now(),
            pinned: true,
        };
        Self {
            messages: vec![system_msg],
            system_prompt: system_prompt.to_string(),
            max_context_tokens,
            max_context_messages,
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: Role::User,
            content: content.to_string(),
            timestamp: Utc::now(),
            pinned: false,
        });
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.to_string(),
            timestamp: Utc::now(),
            pinned: false,
        });
    }

    pub fn get_context_window(&self) -> Vec<&Message> {
        let system = &self.messages[0];
        let non_system: Vec<&Message> = self.messages[1..].iter().collect();

        let max = self.max_context_messages as usize;
        let start = if non_system.len() > max {
            non_system.len() - max
        } else {
            0
        };

        let mut window = vec![system];
        window.extend_from_slice(&non_system[start..]);
        window
    }

    pub fn get_api_messages(&self) -> Vec<(&str, &str)> {
        self.get_context_window()
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                (role, m.content.as_str())
            })
            .collect()
    }

    pub fn estimate_context_tokens(&self) -> u32 {
        let window = self.get_context_window();
        let total_chars: usize = window.iter().map(|m| m.content.len()).sum();
        (total_chars / 4) as u32
    }

    pub fn get_compacted_messages(&self) -> Vec<Message> {
        let max = self.max_context_messages as usize;
        let non_system = &self.messages[1..];

        if non_system.len() <= max {
            return self.messages.clone();
        }

        let cutoff = non_system.len() - max;
        let old_messages = &non_system[..cutoff];
        let recent_messages = &non_system[cutoff..];

        let summary_parts: Vec<String> = old_messages
            .iter()
            .filter(|m| m.role == Role::User)
            .map(|m| {
                if m.content.len() > 80 {
                    format!("- {}...", &m.content[..77])
                } else {
                    format!("- {}", m.content)
                }
            })
            .collect();

        let summary = format!(
            "[Earlier conversation summary ({} messages compacted)]\nTopics discussed:\n{}",
            old_messages.len(),
            summary_parts.join("\n")
        );

        let mut result = vec![self.messages[0].clone()];
        result.push(Message {
            role: Role::Assistant,
            content: summary,
            timestamp: old_messages.last().map(|m| m.timestamp).unwrap_or_else(Utc::now),
            pinned: false,
        });
        result.extend(recent_messages.iter().cloned());
        result
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn all_messages(&self) -> &[Message] {
        &self.messages
    }
}
