use serde_json::json;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OpenSpaceClient {
    base_url: String,
    api_key: String,
}

impl OpenSpaceClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn build_trace_request(&self, start_node: &str) -> (String, String) {
        let url = format!("{}/structural/trace", self.base_url);
        let body = json!({"start_node": start_node}).to_string();
        (url, body)
    }

    pub fn build_impact_request(&self, node: &str) -> (String, String) {
        let url = format!("{}/structural/impact", self.base_url);
        let body = json!({"node": node}).to_string();
        (url, body)
    }

    pub fn build_between_request(&self, from: &str, to: &str) -> (String, String) {
        let url = format!("{}/structural/between", self.base_url);
        let body = json!({"from": from, "to": to}).to_string();
        (url, body)
    }

    pub fn build_categorize_request(&self, node: &str) -> (String, String) {
        let url = format!("{}/structural/categorize", self.base_url);
        let body = json!({"node": node}).to_string();
        (url, body)
    }

    pub fn build_memory_graph_request(&self) -> (String, String) {
        let url = format!("{}/structural/memory_graph", self.base_url);
        let body = json!({}).to_string();
        (url, body)
    }

    pub fn build_extract_request(&self, domain: &str) -> (String, String) {
        let url = format!("{}/structural/extract", self.base_url);
        let body = json!({"domain": domain}).to_string();
        (url, body)
    }
}
