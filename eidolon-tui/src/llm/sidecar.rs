use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarStatus {
    Stopped,
    Starting,
    Ready,
    Error(String),
}

pub struct LlamaSidecar {
    process: Option<Child>,
    port: u16,
    model_path: String,
    server_path: String,
    context_length: u32,
    gpu_layers: u32,
    status: SidecarStatus,
}

impl LlamaSidecar {
    pub fn new(server_path: &str, model_path: &str, port: u16, context_length: u32, gpu_layers: u32) -> Self {
        Self {
            process: None,
            port,
            model_path: model_path.to_string(),
            server_path: server_path.to_string(),
            context_length,
            gpu_layers,
            status: SidecarStatus::Stopped,
        }
    }

    /// Check if llama-server is already running on the configured port.
    pub async fn check_health(&self) -> bool {
        let url = format!("http://localhost:{}/health", self.port);
        let client = reqwest::Client::new();
        tokio::time::timeout(
            std::time::Duration::from_millis(800),
            client.get(&url).send(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|r| r.status().is_success())
        .unwrap_or(false)
    }

    /// Start llama-server if not already running.
    pub async fn start(&mut self) -> Result<(), String> {
        // Check if already running on our port
        if self.check_health().await {
            self.status = SidecarStatus::Ready;
            return Ok(());
        }

        self.status = SidecarStatus::Starting;

        // Kill any stale llama-server from a previous run
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", "llama-server.exe", "/T"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            // Give the OS a moment to release the port
            sleep(Duration::from_millis(500)).await;
        }

        let log_path = dirs::data_dir()
            .unwrap_or_default()
            .join("eidolon")
            .join("llama-server.log");

        let child = Command::new(&self.server_path)
            .args([
                "-m", &self.model_path,
                "-c", &self.context_length.to_string(),
                "-ngl", &self.gpu_layers.to_string(),
                "--port", &self.port.to_string(),
                "--parallel", "2",
            ])
            .stdout(Stdio::null())
            .stderr(
                std::fs::File::create(&log_path)
                    .map(Into::into)
                    .unwrap_or(Stdio::null()),
            )
            .spawn()
            .map_err(|e| format!("Failed to start llama-server: {}", e))?;

        self.process = Some(child);

        // Poll health endpoint until ready (timeout 120s -- 14B model on GPU can take a while)
        for _ in 0..240 {
            sleep(Duration::from_millis(500)).await;

            // Check if process died (port conflict, missing DLL, bad model, etc.)
            if let Some(ref mut child) = self.process {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let msg = format!("llama-server exited during startup ({}). Check {}", status, log_path.display());
                        self.status = SidecarStatus::Error(msg.clone());
                        self.process = None;
                        return Err(msg);
                    }
                    Ok(None) => {} // still running, good
                    Err(e) => {
                        let msg = format!("Failed to check llama-server status: {}", e);
                        self.status = SidecarStatus::Error(msg.clone());
                        return Err(msg);
                    }
                }
            }

            if self.check_health().await {
                self.status = SidecarStatus::Ready;
                return Ok(());
            }
        }

        self.status = SidecarStatus::Error("Startup timeout (120s)".to_string());
        Err("llama-server failed to become healthy within 120 seconds".to_string())
    }

    /// Stop llama-server.
    pub fn stop(&mut self) {
        if let Some(ref mut child) = self.process {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.process = None;
        self.status = SidecarStatus::Stopped;
    }

    pub fn status(&self) -> &SidecarStatus {
        &self.status
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for LlamaSidecar {
    fn drop(&mut self) {
        self.stop();
    }
}
