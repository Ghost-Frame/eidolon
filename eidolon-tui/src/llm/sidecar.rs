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
    context_length: u32,
    gpu_layers: u32,
    status: SidecarStatus,
}

impl LlamaSidecar {
    pub fn new(model_path: &str, port: u16, context_length: u32, gpu_layers: u32) -> Self {
        Self {
            process: None,
            port,
            model_path: model_path.to_string(),
            context_length,
            gpu_layers,
            status: SidecarStatus::Stopped,
        }
    }

    /// Check if llama-server is already running on the configured port.
    pub async fn check_health(&self) -> bool {
        let url = format!("http://localhost:{}/health", self.port);
        match reqwest::get(&url).await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Start llama-server if not already running.
    pub async fn start(&mut self) -> Result<(), String> {
        // Check if already running
        if self.check_health().await {
            self.status = SidecarStatus::Ready;
            return Ok(());
        }

        self.status = SidecarStatus::Starting;

        let child = Command::new("llama-server")
            .args([
                "-m", &self.model_path,
                "-c", &self.context_length.to_string(),
                "-ngl", &self.gpu_layers.to_string(),
                "--port", &self.port.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start llama-server: {}", e))?;

        self.process = Some(child);

        // Poll health endpoint until ready (timeout 30s)
        for _ in 0..60 {
            sleep(Duration::from_millis(500)).await;
            if self.check_health().await {
                self.status = SidecarStatus::Ready;
                return Ok(());
            }
        }

        self.status = SidecarStatus::Error("Startup timeout (30s)".to_string());
        Err("llama-server failed to become healthy within 30 seconds".to_string())
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
