use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct AxonClient {
    base_url: String,
    api_key: String,
}

impl AxonClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn build_publish_request(&self, channel: &str, source: &str, event_type: &str, payload: &Value) -> (String, String) {
        let url = format!("{}/axon/publish", self.base_url);
        let body = json!({
            "channel": channel,
            "source": source,
            "type": event_type,
            "payload": payload
        }).to_string();
        (url, body)
    }

    pub fn build_events_url(&self, channel: &str, limit: u32) -> String {
        format!("{}/axon/events?channel={}&limit={}", self.base_url, channel, limit)
    }

    pub fn build_subscribe_request(&self, agent: &str, channel: &str) -> (String, String) {
        let url = format!("{}/axon/subscribe", self.base_url);
        let body = json!({
            "agent": agent,
            "channel": channel
        }).to_string();
        (url, body)
    }
}
