use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::sync::Mutex;

/// Tracks per-session state for the proxy pipeline.
/// Handles conversation diffing and injection staleness checks.
pub struct ProxySessionTracker {
    sessions: Mutex<HashMap<String, ProxySession>>,
}

struct ProxySession {
    /// Number of messages seen so far (used for diffing)
    seen_message_count: usize,
    /// Last injected context (for differential check)
    last_injection: Option<String>,
    /// Memories stored this session (for volume cap)
    memories_stored: usize,
    /// When this session was last active
    last_active: Instant,
}

impl ProxySessionTracker {
    pub fn new() -> Self {
        ProxySessionTracker {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Get new messages since the last time we processed this session.
    /// Returns the new messages and updates the seen count.
    pub async fn get_new_turns(
        &self,
        session_id: &str,
        messages: &[serde_json::Value],
    ) -> Vec<serde_json::Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.entry(session_id.to_string()).or_insert_with(|| ProxySession {
            seen_message_count: 0,
            last_injection: None,
            memories_stored: 0,
            last_active: Instant::now(),
        });

        let prev_count = session.seen_message_count;
        session.seen_message_count = messages.len();
        session.last_active = Instant::now();

        if prev_count >= messages.len() {
            return Vec::new();
        }

        messages[prev_count..].to_vec()
    }

    /// Check if the new injection content is too similar to the last one.
    /// Returns true if the content is stale (should skip injection).
    pub async fn check_staleness(
        &self,
        session_id: &str,
        new_content: &str,
        threshold: f32,
    ) -> bool {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(session_id) {
            if let Some(ref last) = session.last_injection {
                return jaccard_similarity(last, new_content) > threshold;
            }
        }
        false
    }

    /// Update the last injection content for this session.
    pub async fn update_last_injection(&self, session_id: &str, content: String) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_injection = Some(content);
        }
    }

    /// Increment the memories stored count. Returns the new total.
    pub async fn increment_memories(&self, session_id: &str) -> usize {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.memories_stored += 1;
            session.memories_stored
        } else {
            0
        }
    }

    /// Get the number of memories stored in this session.
    pub async fn memories_stored(&self, session_id: &str) -> usize {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).map(|s| s.memories_stored).unwrap_or(0)
    }

    /// Clean up sessions older than the given duration.
    pub async fn evict_stale(&self, max_age: std::time::Duration) {
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, s| s.last_active.elapsed() < max_age);
    }

    /// Get stats for observability.
    pub async fn stats(&self) -> serde_json::Value {
        let sessions = self.sessions.lock().await;
        let total = sessions.len();
        let total_memories: usize = sessions.values().map(|s| s.memories_stored).sum();
        serde_json::json!({
            "active_sessions": total,
            "total_memories_stored": total_memories,
        })
    }
}

/// Token-level Jaccard similarity between two strings.
/// Splits on whitespace, computes |intersection| / |union|.
fn jaccard_similarity(a: &str, b: &str) -> f32 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        return 1.0;
    }

    intersection as f32 / union as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaccard_identical() {
        assert!((jaccard_similarity("hello world", "hello world") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_jaccard_disjoint() {
        assert!((jaccard_similarity("hello world", "foo bar") - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_jaccard_partial() {
        let sim = jaccard_similarity("hello world foo", "hello world bar");
        assert!(sim > 0.4 && sim < 0.7); // 2 shared out of 4 unique
    }

    #[test]
    fn test_jaccard_empty() {
        assert!((jaccard_similarity("", "") - 1.0).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_session_tracker_new_turns() {
        let tracker = ProxySessionTracker::new();
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "hi"}),
        ];

        // First call -- all messages are new
        let new = tracker.get_new_turns("s1", &msgs).await;
        assert_eq!(new.len(), 2);

        // Second call with same messages -- nothing new
        let new = tracker.get_new_turns("s1", &msgs).await;
        assert_eq!(new.len(), 0);

        // Third call with one new message
        let mut msgs2 = msgs.clone();
        msgs2.push(serde_json::json!({"role": "user", "content": "how are you?"}));
        let new = tracker.get_new_turns("s1", &msgs2).await;
        assert_eq!(new.len(), 1);
    }

    #[tokio::test]
    async fn test_staleness_check() {
        let tracker = ProxySessionTracker::new();
        // Initialize session
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        tracker.get_new_turns("s1", &msgs).await;

        // No prior injection -- not stale
        assert!(!tracker.check_staleness("s1", "some context", 0.8).await);

        // Set injection
        tracker.update_last_injection("s1", "some context here".to_string()).await;

        // Same content -- stale
        assert!(tracker.check_staleness("s1", "some context here", 0.8).await);

        // Very different content -- not stale
        assert!(!tracker.check_staleness("s1", "completely different topic", 0.8).await);
    }
}
