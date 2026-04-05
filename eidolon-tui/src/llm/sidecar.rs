use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarStatus {
    Stopped,
    Starting,
    Ready,
    Degraded(String), // Was ready, now unhealthy
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
    degraded_since: Option<Instant>,
}

// ---------------------------------------------------------------------------
// PID file helpers
// ---------------------------------------------------------------------------

fn pid_file_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("eidolon")
        .join("llama.pid")
}

fn write_pid_file(pid: u32) {
    let path = pid_file_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, pid.to_string());
}

fn read_pid_file() -> Option<u32> {
    fs::read_to_string(pid_file_path())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

fn remove_pid_file() {
    let _ = fs::remove_file(pid_file_path());
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        // tasklist /FI "PID eq <pid>" /NH returns a header-less table.
        // If the process exists the output contains the pid string.
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        // kill -0 checks existence without sending a real signal.
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Targeted kill helpers
// ---------------------------------------------------------------------------

fn kill_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // SIGTERM first, SIGKILL after 5 s if still alive.
        let _ = Command::new("kill")
            .args([&pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        std::thread::sleep(Duration::from_secs(5));

        if is_process_alive(pid) {
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

// ---------------------------------------------------------------------------
// Port helpers
// ---------------------------------------------------------------------------

fn find_available_port(preferred: u16) -> u16 {
    if TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return preferred;
    }
    for offset in 1..=20u16 {
        let candidate = preferred.wrapping_add(offset);
        if TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return candidate;
        }
    }
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(preferred)
}

// ---------------------------------------------------------------------------
// LlamaSidecar
// ---------------------------------------------------------------------------

impl LlamaSidecar {
    pub fn new(
        server_path: &str,
        model_path: &str,
        port: u16,
        context_length: u32,
        gpu_layers: u32,
    ) -> Self {
        Self {
            process: None,
            port,
            model_path: model_path.to_string(),
            server_path: server_path.to_string(),
            context_length,
            gpu_layers,
            status: SidecarStatus::Stopped,
            degraded_since: None,
        }
    }

    // -----------------------------------------------------------------------
    // Public accessors
    // -----------------------------------------------------------------------

    pub fn status(&self) -> &SidecarStatus {
        &self.status
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().and_then(|c| {
            // `id()` is infallible on std::process::Child once spawned.
            Some(c.id())
        })
    }

    // -----------------------------------------------------------------------
    // Health checks
    // -----------------------------------------------------------------------

    /// Single-shot health check against the configured port.
    pub async fn check_health(&self) -> bool {
        let url = format!("http://localhost:{}/health", self.port);
        let client = reqwest::Client::new();
        tokio::time::timeout(
            Duration::from_millis(800),
            client.get(&url).send(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|r| r.status().is_success())
        .unwrap_or(false)
    }

    /// Health check with exponential backoff.
    /// Starts at 200 ms, doubles each attempt, caps at 4 s, total timeout 120 s.
    async fn check_health_with_backoff(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut interval = Duration::from_millis(200);
        let max_interval = Duration::from_secs(4);

        let log_path = dirs::data_local_dir()
            .unwrap_or_default()
            .join("eidolon")
            .join("llama-server.log");

        loop {
            if Instant::now() >= deadline {
                return false;
            }

            sleep(interval).await;
            interval = (interval * 2).min(max_interval);

            // Bail early if the child process already died.
            if let Some(ref mut child) = self.process {
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        let msg = format!(
                            "llama-server exited during startup ({}). Check {}",
                            exit_status,
                            log_path.display()
                        );
                        self.status = SidecarStatus::Error(msg);
                        self.process = None;
                        remove_pid_file();
                        return false;
                    }
                    Ok(None) => {} // still running
                    Err(e) => {
                        self.status =
                            SidecarStatus::Error(format!("Failed to check process status: {}", e));
                        return false;
                    }
                }
            }

            if self.check_health().await {
                return true;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Recovery
    // -----------------------------------------------------------------------

    /// Called periodically by the owner. Transitions between Ready/Degraded/Error
    /// and attempts a restart if Degraded for more than 30 s.
    pub async fn check_and_recover(&mut self) -> SidecarStatus {
        match &self.status {
            SidecarStatus::Ready => {
                if !self.check_health().await {
                    let msg = "health check failed".to_string();
                    self.status = SidecarStatus::Degraded(msg);
                    self.degraded_since = Some(Instant::now());
                }
            }
            SidecarStatus::Degraded(_) => {
                if self.check_health().await {
                    // Recovered on its own.
                    self.status = SidecarStatus::Ready;
                    self.degraded_since = None;
                } else {
                    let elapsed = self
                        .degraded_since
                        .map(|t| t.elapsed())
                        .unwrap_or(Duration::from_secs(31));

                    if elapsed > Duration::from_secs(30) {
                        // Attempt restart.
                        self.cleanup();
                        self.status = SidecarStatus::Starting;
                        self.degraded_since = None;
                        match self.start().await {
                            Ok(()) => {} // status already set to Ready inside start()
                            Err(e) => {
                                self.status = SidecarStatus::Error(format!("Restart failed: {}", e));
                            }
                        }
                    }
                }
            }
            // Nothing to do for Stopped / Starting / Error.
            _ => {}
        }

        self.status.clone()
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Start llama-server.
    ///
    /// 1. If already healthy on our port, mark Ready and return.
    /// 2. Kill any stale process from the PID file.
    /// 3. Find a free port.
    /// 4. Spawn the server, write PID file.
    /// 5. Poll health with exponential backoff (up to 120 s).
    pub async fn start(&mut self) -> Result<(), String> {
        // Already healthy -- adopt it without spawning a new one.
        if self.check_health().await {
            self.status = SidecarStatus::Ready;
            return Ok(());
        }

        self.status = SidecarStatus::Starting;

        // Kill stale process from previous run.
        if let Some(stale_pid) = read_pid_file() {
            if is_process_alive(stale_pid) {
                kill_pid(stale_pid);
                // Brief pause to let the OS release the port.
                sleep(Duration::from_millis(300)).await;
            }
            remove_pid_file();
        }

        self.port = find_available_port(self.port);

        let log_path = dirs::data_local_dir()
            .unwrap_or_default()
            .join("eidolon")
            .join("llama-server.log");

        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let child = Command::new(&self.server_path)
            .args([
                "-m",
                &self.model_path,
                "-c",
                &self.context_length.to_string(),
                "-ngl",
                &self.gpu_layers.to_string(),
                "--port",
                &self.port.to_string(),
                "--parallel",
                "2",
            ])
            .stdout(Stdio::null())
            .stderr(
                fs::File::create(&log_path)
                    .map(Into::into)
                    .unwrap_or_else(|_| Stdio::null()),
            )
            .spawn()
            .map_err(|e| format!("Failed to start llama-server: {}", e))?;

        let pid = child.id();
        self.process = Some(child);
        write_pid_file(pid);

        if self.check_health_with_backoff().await {
            self.status = SidecarStatus::Ready;
            Ok(())
        } else {
            // check_health_with_backoff may have already set a more specific error.
            if matches!(self.status, SidecarStatus::Starting) {
                self.status =
                    SidecarStatus::Error("Startup timeout (120s)".to_string());
            }
            Err(format!(
                "llama-server failed to become healthy within 120 seconds. Check {}",
                log_path.display()
            ))
        }
    }

    /// Kill the managed process by PID and remove the PID file.
    /// Safe to call multiple times.
    pub fn cleanup(&mut self) {
        // Prefer killing by tracked PID for precision.
        let pid = self.pid().or_else(read_pid_file);

        // Drop the Child handle first so we own the pid cleanly.
        if let Some(ref mut child) = self.process {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.process = None;

        if let Some(pid) = pid {
            if is_process_alive(pid) {
                kill_pid(pid);
            }
        }

        remove_pid_file();
        self.status = SidecarStatus::Stopped;
        self.degraded_since = None;
    }

    /// Static cleanup via PID file only -- for use in panic hooks where
    /// we don't have access to the sidecar instance.
    pub fn cleanup_by_pid_file() {
        if let Some(pid) = read_pid_file() {
            if is_process_alive(pid) {
                kill_pid(pid);
            }
            remove_pid_file();
        }
    }
}

impl Drop for LlamaSidecar {
    fn drop(&mut self) {
        self.cleanup();
    }
}
