use crate::agents::claude::ClaudeSession;
use crate::agents::codex::CodexSession;
use crate::config::AgentsConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

pub enum AgentType {
    Claude { model: String },
    Codex,
}

pub struct ActiveSession {
    pub id: String,
    pub agent_type: String,
    pub task: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub output_rx: mpsc::UnboundedReceiver<String>,
}

pub struct AgentOrchestrator {
    config: AgentsConfig,
    claude_sessions: HashMap<String, ClaudeSession>,
    codex_sessions: HashMap<String, CodexSession>,
}

impl AgentOrchestrator {
    pub fn new(config: AgentsConfig) -> Self {
        Self {
            config,
            claude_sessions: HashMap::new(),
            codex_sessions: HashMap::new(),
        }
    }

    pub async fn spawn(
        &mut self,
        agent_type: AgentType,
        task: &str,
        living_prompt: Option<&str>,
        working_dir: Option<PathBuf>,
    ) -> Result<ActiveSession, String> {
        let session_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();

        match agent_type {
            AgentType::Claude { model } => {
                let mut session = ClaudeSession::new(&session_id, task, &model);

                if let Some(prompt) = living_prompt {
                    session.write_living_prompt(prompt)
                        .map_err(|e| format!("Failed to write living prompt: {}", e))?;
                }

                session.spawn(&self.config.claude.command, &self.config.claude.args, tx).await?;
                let agent_type_str = format!("claude-{}", model);
                self.claude_sessions.insert(session_id.clone(), session);

                Ok(ActiveSession {
                    id: session_id,
                    agent_type: agent_type_str,
                    task: task.to_string(),
                    started_at: chrono::Utc::now(),
                    output_rx: rx,
                })
            }
            AgentType::Codex => {
                let dir = working_dir.unwrap_or_else(|| PathBuf::from("."));
                let mut session = CodexSession::new(&session_id, task, dir);
                session.spawn(&self.config.codex.command, &self.config.codex.args, tx).await?;

                self.codex_sessions.insert(session_id.clone(), session);

                Ok(ActiveSession {
                    id: session_id,
                    agent_type: "codex".to_string(),
                    task: task.to_string(),
                    started_at: chrono::Utc::now(),
                    output_rx: rx,
                })
            }
        }
    }

    pub fn kill_session(&mut self, session_id: &str) {
        if let Some(session) = self.claude_sessions.get_mut(session_id) {
            session.kill();
        }
        if let Some(session) = self.codex_sessions.get_mut(session_id) {
            session.kill();
        }
    }

    pub fn active_sessions(&mut self) -> Vec<String> {
        let mut active = Vec::new();
        for (id, session) in &mut self.claude_sessions {
            if session.is_running() {
                active.push(id.clone());
            }
        }
        for (id, session) in &mut self.codex_sessions {
            if session.is_running() {
                active.push(id.clone());
            }
        }
        active
    }
}
