use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

pub struct ClaudeSession {
    pub session_id: String,
    pub task: String,
    pub model: String,
    process: Option<Child>,
    session_dir: PathBuf,
}

impl ClaudeSession {
    pub fn new(session_id: &str, task: &str, model: &str) -> Self {
        let session_dir = std::env::temp_dir()
            .join("eidolon-sessions")
            .join(session_id);
        Self {
            session_id: session_id.to_string(),
            task: task.to_string(),
            model: model.to_string(),
            process: None,
            session_dir,
        }
    }

    pub fn write_living_prompt(&self, prompt_content: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.session_dir)?;
        let claude_md = self.session_dir.join("CLAUDE.md");
        std::fs::write(claude_md, prompt_content)?;
        Ok(())
    }

    pub async fn spawn(
        &mut self,
        command: &str,
        args: &[String],
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .arg(&self.task)
            .current_dir(&self.session_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let stdout = child.stdout.take().ok_or("No stdout")?;
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx_clone.send(line);
            }
        });

        self.process = Some(child);
        Ok(())
    }

    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.process {
            let _ = child.start_kill();
        }
    }

    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(None) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    pub async fn wait(&mut self) -> Option<i32> {
        if let Some(ref mut child) = self.process {
            match child.wait().await {
                Ok(status) => status.code(),
                Err(_) => None,
            }
        } else {
            None
        }
    }
}
