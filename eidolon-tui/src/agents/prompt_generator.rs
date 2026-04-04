// src/agents/prompt_generator.rs
use crate::syntheos::engram::EngramClient;
use crate::syntheos::openspace::OpenSpaceClient;
use crate::syntheos::chiasm::ChiasmClient;
use crate::syntheos::broca::BrocaClient;

/// Assembles a dynamic CLAUDE.md for spawned agent sessions.
/// Queries Engram, OpenSpace, Chiasm, and Broca for relevant context.
pub struct PromptGenerator {
    engram: EngramClient,
    openspace: OpenSpaceClient,
    chiasm: ChiasmClient,
    broca: BrocaClient,
    http: reqwest::Client,
    api_key: String,
}

impl PromptGenerator {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            engram: EngramClient::new(base_url, api_key)
                .unwrap_or_else(|_| panic!("Invalid Engram URL: {}", base_url)),
            openspace: OpenSpaceClient::new(base_url, api_key),
            chiasm: ChiasmClient::new(base_url, api_key),
            broca: BrocaClient::new(base_url, api_key),
            http: reqwest::Client::new(),
            api_key: api_key.to_string(),
        }
    }

    /// Generate a complete living prompt for a task.
    pub async fn generate(&self, task: &str) -> String {
        let memories = self.fetch_memories(task).await;
        let active_tasks = self.fetch_active_tasks().await;
        let recent_activity = self.fetch_recent_activity().await;
        let graph_context = self.fetch_graph_context(task).await;

        format!(
            r#"# Eidolon Session Context
Generated: {}
Task: {}

## Relevant Context
{}

## Active Work
{}

## Recent Activity
{}

## Graph Context
{}

## Safety Rules
- NEVER assign passwords - ask the user
- NEVER hardcode credentials - use cred/credd
- NEVER modify SSH config without verifying access first
- NEVER force push to main/master without explicit approval
- NEVER run destructive commands without checking state first
- Use SSH config aliases for server access
- Verify before acting: check state -> act -> verify result

## Syntheos APIs
All services at $ENGRAM_URL with Bearer $ENGRAM_API_KEY:
- /search, /store, /recall, /context (Engram memory)
- /tasks (Chiasm task tracking)
- /broca/ask, /broca/actions, /broca/feed (Action log)
- /axon/publish, /axon/events (Event bus)
- /soma/agents (Agent registry)
"#,
            chrono::Utc::now().to_rfc3339(),
            task,
            memories,
            active_tasks,
            recent_activity,
            graph_context,
        )
    }

    async fn fetch_memories(&self, query: &str) -> String {
        let (url, body) = self.engram.build_search_request(query, 10);
        match self.post(&url, &body).await {
            Ok(resp) => Self::format_memories(&resp),
            Err(_) => "(Engram unavailable)".to_string(),
        }
    }

    async fn fetch_active_tasks(&self) -> String {
        let url = self.chiasm.build_list_tasks_url();
        match self.get(&url).await {
            Ok(resp) => Self::format_tasks(&resp),
            Err(_) => "(Chiasm unavailable)".to_string(),
        }
    }

    async fn fetch_recent_activity(&self) -> String {
        let url = self.broca.build_feed_url(10);
        match self.get(&url).await {
            Ok(resp) => Self::format_activity(&resp),
            Err(_) => "(Broca unavailable)".to_string(),
        }
    }

    async fn fetch_graph_context(&self, _task: &str) -> String {
        let (url, body) = self.openspace.build_memory_graph_request();
        match self.post(&url, &body).await {
            Ok(resp) => Self::format_graph(&resp),
            Err(_) => "(OpenSpace unavailable)".to_string(),
        }
    }

    fn format_memories(raw: &str) -> String {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(results) = val.as_array().or_else(|| val.get("results").and_then(|r| r.as_array())) {
                return results.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|c| format!("- {}", c))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        raw.to_string()
    }

    fn format_tasks(raw: &str) -> String {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(tasks) = val.as_array().or_else(|| val.get("tasks").and_then(|t| t.as_array())) {
                return tasks.iter()
                    .filter_map(|t| {
                        let title = t.get("title")?.as_str()?;
                        let status = t.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
                        let agent = t.get("agent").and_then(|a| a.as_str()).unwrap_or("?");
                        Some(format!("- [{}] {} ({})", status, title, agent))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        raw.to_string()
    }

    fn format_activity(raw: &str) -> String {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(items) = val.as_array().or_else(|| val.get("actions").and_then(|a| a.as_array())) {
                return items.iter()
                    .take(5)
                    .filter_map(|a| {
                        let action = a.get("action")?.as_str()?;
                        let agent = a.get("agent").and_then(|ag| ag.as_str()).unwrap_or("?");
                        Some(format!("- {} ({})", action, agent))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        raw.to_string()
    }

    fn format_graph(raw: &str) -> String {
        if raw.len() > 500 {
            let truncated: String = raw.chars().take(497).collect();
            format!("{}... (truncated)", truncated)
        } else {
            raw.to_string()
        }
    }

    async fn post(&self, url: &str, body: &str) -> Result<String, reqwest::Error> {
        self.http.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send().await?
            .text().await
    }

    async fn get(&self, url: &str) -> Result<String, reqwest::Error> {
        self.http.get(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send().await?
            .text().await
    }
}
