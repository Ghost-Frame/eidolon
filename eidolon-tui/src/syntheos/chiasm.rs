use serde_json::json;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ChiasmClient {
    base_url: String,
    api_key: String,
}

impl ChiasmClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn build_create_task_request(&self, agent: &str, project: &str, title: &str) -> (String, String) {
        let url = format!("{}/tasks", self.base_url);
        let body = json!({
            "agent": agent,
            "project": project,
            "title": title
        }).to_string();
        (url, body)
    }

    pub fn build_update_task_request(&self, task_id: u64, status: &str, summary: &str) -> (String, String) {
        let url = format!("{}/tasks/{}", self.base_url, task_id);
        let body = json!({
            "status": status,
            "summary": summary
        }).to_string();
        (url, body)
    }

    pub fn build_list_tasks_url(&self) -> String {
        format!("{}/tasks", self.base_url)
    }

    pub fn build_feed_url(&self) -> String {
        format!("{}/feed", self.base_url)
    }
}
