//! Child process supervisor.
//!
//! Manages two long-lived child processes:
//!
//! 1. The OpenChamber web server (`node packages/web/dist-server/index.js`
//!    or, in dev mode, `node packages/web/server/index.js`). It binds to
//!    `127.0.0.1:<chosen>` and hosts the React UI plus the
//!    `/api/opencode` reverse proxy.
//!
//! 2. The OpenCode CLI server (`opencode serve --port <port> --hostname
//!    127.0.0.1`). The OpenChamber web server talks to it over HTTP/SSE.
//!
//! Both are spawned at app startup, supervised until the parent exits, and
//! killed on shutdown. Health is polled by issuing a GET to the published
//! `/health` and `/global/health` endpoints respectively.
//!
//! Port selection prefers an env override (`OPENCHAMBER_PORT`,
//! `OPENCODE_PORT`), then `pick_free_port()` which talks to the kernel via
//! `bind(0)` / `listen()` / `close()`.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct ServerEndpoints {
    pub openchamber_url: String,
    pub opencode_url: String,
    pub opencode_auth: Option<String>,
}

pub struct Supervisor {
    inner: Mutex<SupervisorInner>,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self {
            inner: Mutex::new(SupervisorInner {
                openchamber: None,
                opencode: None,
            }),
        }
    }
}

struct SupervisorInner {
    openchamber: Option<Child>,
    opencode: Option<Child>,
}

impl Supervisor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Start both children. Returns the resolved endpoints once both health
    /// probes succeed (or the configured timeout elapses).
    pub async fn start(&self, openchamber_url: String, opencode_url: String) -> Result<ServerEndpoints> {
        let openchamber_port = port_from_url(&openchamber_url)?;
        let opencode_port = port_from_url(&opencode_url)?;

        if env_flag("OPENCODE_SKIP_START") {
            log::info!("[supervisor] OPENCODE_SKIP_START set, skipping opencode child spawn");
        } else {
            self.spawn_opencode(opencode_port).await?;
        }

        if env_flag("OPENCHAMBER_SKIP_LOCAL_SERVER") {
            log::info!(
                "[supervisor] OPENCHAMBER_SKIP_LOCAL_SERVER set, skipping openchamber child spawn"
            );
        } else {
            self.spawn_openchamber(openchamber_port).await?;
        }

        wait_for_health(&openchamber_url, "/health", Duration::from_secs(20)).await?;
        wait_for_health(&opencode_url, "/global/health", Duration::from_secs(10)).await?;

        Ok(ServerEndpoints {
            openchamber_url,
            opencode_url,
            opencode_auth: std::env::var("OPENCODE_SERVER_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }

    async fn spawn_opencode(&self, port: u16) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if inner.opencode.is_some() {
            return Ok(());
        }

        let binary = resolve_opencode_binary().context("could not locate opencode binary")?;
        log::info!("[supervisor] spawning opencode from {}", binary.display());

        let password = std::env::var("OPENCODE_SERVER_PASSWORD").unwrap_or_default();
        let mut cmd = Command::new(&binary);
        cmd.arg("serve")
            .arg("--port").arg(port.to_string())
            .arg("--hostname").arg("127.0.0.1")
            .env("OPENCODE_SERVER_PASSWORD", password)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().context("failed to spawn opencode serve")?;
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(log_lines("opencode", stdout));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(log_lines("opencode:err", stderr));
        }
        inner.opencode = Some(child);
        Ok(())
    }

    async fn spawn_openchamber(&self, port: u16) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if inner.openchamber.is_some() {
            return Ok(());
        }

        let (program, args) = resolve_openchamber_launcher(port)?;
        log::info!(
            "[supervisor] spawning openchamber server: {} {}",
            program,
            args.join(" ")
        );

        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().context("failed to spawn openchamber server")?;
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(log_lines("openchamber", stdout));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(log_lines("openchamber:err", stderr));
        }
        inner.openchamber = Some(child);
        Ok(())
    }

    /// Kill both children and wait briefly for them to exit.
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(mut child) = inner.opencode.take() {
            log::info!("[supervisor] stopping opencode");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(mut child) = inner.openchamber.take() {
            log::info!("[supervisor] stopping openchamber");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

async fn log_lines<R>(label: &'static str, reader: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        log::info!("[{}] {}", label, line);
    }
}

async fn wait_for_health(base: &str, path: &str, timeout: Duration) -> Result<()> {
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let deadline = Instant::now() + timeout;
    let mut delay = Duration::from_millis(250);
    loop {
        if matches!(probe_health(&url).await, Ok(true)) {
            log::info!("[supervisor] {} is ready", url);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("timeout waiting for {}", url));
        }
        tokio::time::sleep(delay).await;
        delay = std::cmp::min(delay * 2, Duration::from_secs(2));
    }
}

async fn probe_health(url: &str) -> Result<bool> {
    let parsed = url::Url::parse(url).context("invalid health url")?;
    let host = parsed.host_str().unwrap_or("127.0.0.1");
    let port = parsed.port_or_known_default().unwrap_or(80);
    let path = parsed.path();
    let mut stream = TcpStream::connect((host, port))
        .await
        .context("tcp connect")?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.ok();
    let mut buf = Vec::with_capacity(256);
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_buf(&mut buf)).await;
    let head = String::from_utf8_lossy(&buf);
    Ok(head.contains(" 200 ") || head.contains(" 204 "))
}

fn port_from_url(url: &str) -> Result<u16> {
    let parsed = url::Url::parse(url).context("invalid server url")?;
    parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow!("url is missing a port: {}", url))
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().map(|v| v.to_ascii_lowercase()),
        Some(ref v) if matches!(v.as_str(), "1" | "true" | "yes")
    )
}

fn resolve_opencode_binary() -> Result<PathBuf> {
    if let Ok(env) = std::env::var("OPENCODE_BINARY") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
    }
    for var in ["OPENCODE_PATH", "OPENCHAMBER_OPENCODE_PATH", "OPENCHAMBER_OPENCODE_BIN"] {
        if let Ok(env) = std::env::var(var) {
            let p = PathBuf::from(env);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    if let Ok(resources) = std::env::var("TAURI_RESOURCES_DIR") {
        let p = PathBuf::from(resources)
            .join("opencode-cli")
            .join(binary_name("opencode"));
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Some(p) = which(&binary_name("opencode")) {
        return Ok(p);
    }
    Err(anyhow!("opencode binary not found"))
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    }
}

fn resolve_openchamber_launcher(port: u16) -> Result<(String, Vec<String>)> {
    if let Ok(server_dir) = std::env::var("OPENCHAMBER_SERVER_DIR") {
        let entry = PathBuf::from(&server_dir).join("index.js");
        if entry.is_file() {
            return Ok((
                std::env::var("OPENCHAMBER_SERVER_NODE").unwrap_or_else(|_| "node".to_string()),
                vec![
                    entry.to_string_lossy().to_string(),
                    "--port".to_string(),
                    port.to_string(),
                ],
            ));
        }
    }

    // Dev mode: workspace root is two levels up from src-tauri, i.e. the
    // `packages/` dir. NOTE: CARGO_MANIFEST_DIR only exists at compile
    // time, so bake it in with env!() — std::env::var is empty at runtime.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let packages_dir = PathBuf::from(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_default();
    let dev_script = packages_dir.join("web/server/index.js");
    if dev_script.is_file() {
        let program = std::env::var("OPENCHAMBER_SERVER_NODE").unwrap_or_else(|_| "node".to_string());
        return Ok((
            program,
            vec![
                dev_script.to_string_lossy().to_string(),
                "--port".to_string(),
                port.to_string(),
                "--cors".to_string(),
                "tauri://localhost".to_string(),
                "--cors".to_string(),
                "http://localhost:5173".to_string(),
            ],
        ));
    }
    Err(anyhow!(
        "could not locate openchamber server entry point (set OPENCHAMBER_SERVER_DIR or run from repo root)"
    ))
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Extension trait to grab the shared `Supervisor` from any `AppHandle`.
pub trait SupervisorExt {
    fn supervisor(&self) -> Arc<Supervisor>;
}

impl<R: tauri::Runtime> SupervisorExt for AppHandle<R> {
    fn supervisor(&self) -> Arc<Supervisor> {
        self.state::<Arc<Supervisor>>().inner().clone()
    }
}