use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

pub struct CodexSession {
    pub session_id: String,
    pub task: String,
    pub model: String,
    process: Option<Child>,
    working_dir: PathBuf,
}

impl CodexSession {
    pub fn new(session_id: &str, task: &str, model: &str, working_dir: PathBuf) -> Self {
        Self {
            session_id: session_id.to_string(),
            task: task.to_string(),
            model: model.to_string(),
            process: None,
            working_dir,
        }
    }

    pub async fn spawn(
        &mut self,
        command: &str,
        args: &[String],
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), String> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        if !self.model.is_empty() {
            cmd.args(["--model", &self.model]);
        }
        cmd.arg(&self.task)
            .current_dir(&self.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn codex: {}", e))?;

        let stdout = child.stdout.take().ok_or("No stdout")?;
        let tx_clone = tx.clone();

        let stderr = child.stderr.take();
        let tx_err = tx_clone.clone();

        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx_clone.send(line + "\n");
            }
        });

        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_err.send(line + "\n");
                }
            });
        }

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
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    pub async fn wait(&mut self) -> Option<i32> {
        if let Some(ref mut child) = self.process {
            child.wait().await.ok().and_then(|s| s.code())
        } else {
            None
        }
    }
}
