//! Runtime configuration injection.
//!
//! The upstream Electron main process injects a small `<script>` into the
//! served `index.html` so the renderer knows the local API base URL, the
//! client token minted at startup, runtime headers, and the user's home
//! directory. Tauri doesn't have an HTTP front door in dev mode (it loads the
//! Vite dev URL directly), so we expose the same data via the `initialization_script`
//! hook on the WebviewWindow. In packaged builds the renderer asks for it
//! through `invoke('get_runtime_config')` instead, because the static
//! `web-dist/index.html` can't be mutated at runtime.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeConfig {
    pub local_origin: String,
    pub api_base_url: String,
    pub client_token: String,
    pub runtime_headers: serde_json::Value,
    pub relay_host_id: String,
    pub home_directory: String,
    pub macos_major: Option<u32>,
    pub platform: &'static str,
    pub arch: String,
    pub tray_enabled: bool,
    pub app_version: String,
}

impl RuntimeConfig {
    pub fn for_local_origin(local_origin: String, port: u16) -> Self {
        Self {
            local_origin: local_origin.clone(),
            api_base_url: local_origin,
            client_token: String::new(),
            runtime_headers: serde_json::Value::Object(Default::default()),
            relay_host_id: String::new(),
            home_directory: std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_default(),
            macos_major: detect_macos_major(),
            platform: std_detect_platform(),
            arch: std::env::consts::ARCH.to_string(),
            tray_enabled: cfg!(target_os = "macos") || cfg!(target_os = "linux") || cfg!(target_os = "windows"),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

fn detect_macos_major() -> Option<u32> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    let major: u32 = s.trim().split('.').next()?.parse().ok()?;
    Some(major)
}

fn std_detect_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

pub fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}