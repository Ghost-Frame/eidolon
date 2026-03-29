use std::collections::HashMap;

pub struct ScrubRegistry {
    tracked: HashMap<String, Vec<String>>,
}

impl ScrubRegistry {
    pub fn new() -> Self {
        ScrubRegistry {
            tracked: HashMap::new(),
        }
    }

    pub fn track(&mut self, session_id: &str, value: String) {
        self.tracked
            .entry(session_id.to_string())
            .or_default()
            .push(value);
    }

    pub fn scrub(&self, session_id: &str, text: &str) -> String {
        let Some(values) = self.tracked.get(session_id) else {
            return text.to_string();
        };
        let mut result = text.to_string();
        for secret in values {
            if !secret.is_empty() {
                result = result.replace(secret, "[REDACTED]");
            }
        }
        result
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.tracked.remove(session_id);
    }
}
