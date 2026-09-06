//! Runtime commands — these mirror Electron main's global identity, log
//! surface, and quit/restart flow. They are always safe to invoke.

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::runtime_config::RuntimeConfig;

/// Returned to the renderer at startup. Equivalent to the Electron
/// `__OPENCHAMBER_ELECTRON__` / `__OPENCHAMBER_PLATFORM__` / `__OPENCHAMBER_HOME__`
/// globals.
#[tauri::command]
pub fn get_runtime_config(state: tauri::State<RuntimeConfig>) -> RuntimeConfig {
    state.inner().clone()
}

/// Inject the same globals the Electron main process inlined into `index.html`.
/// Dev mode uses this through `WebviewWindowBuilder::initialization_script`,
/// but packaged builds reach it through `invoke('get_runtime_config')` and
/// apply it client-side via the bridge adapter.
pub fn build_initialization_script(config: &RuntimeConfig) -> String {
    let local_origin = serde_json::to_string(&config.local_origin).expect("serializable");
    let api_base_url = serde_json::to_string(&config.api_base_url).expect("serializable");
    let client_token = serde_json::to_string(&config.client_token).expect("serializable");
    let runtime_headers = serde_json::to_string(&config.runtime_headers).expect("serializable");
    let relay_host_id = serde_json::to_string(&config.relay_host_id).expect("serializable");
    let home = serde_json::to_string(&config.home_directory).expect("serializable");
    let platform = serde_json::to_string(&config.platform).expect("serializable");
    let arch = serde_json::to_string(&config.arch).expect("serializable");
    let version = serde_json::to_string(&config.app_version).expect("serializable");
    let tray_enabled = if config.tray_enabled { "true" } else { "false" };
    let macos_major = config
        .macos_major
        .map(|m| m.to_string())
        .unwrap_or_else(|| "null".to_string());

    format!(
        r#"
        (function () {{
            if (window.__OPENCHAMBER_LOCAL_ORIGIN__ === undefined) {{
                window.__OPENCHAMBER_LOCAL_ORIGIN__ = {local_origin};
            }}
            if (window.__OPENCHAMBER_API_BASE_URL__ === undefined) {{
                window.__OPENCHAMBER_API_BASE_URL__ = {api_base_url};
            }}
            if ({client_token} && window.__OPENCHAMBER_CLIENT_TOKEN__ === undefined) {{
                window.__OPENCHAMBER_CLIENT_TOKEN__ = {client_token};
            }}
            if (window.__OPENCHAMBER_RUNTIME_HEADERS__ === undefined) {{
                window.__OPENCHAMBER_RUNTIME_HEADERS__ = {runtime_headers};
            }}
            if ({relay_host_id} && window.__OPENCHAMBER_RELAY_HOST_ID__ === undefined) {{
                window.__OPENCHAMBER_RELAY_HOST_ID__ = {relay_host_id};
            }}
            if ({home} && window.__OPENCHAMBER_HOME__ === undefined) {{
                window.__OPENCHAMBER_HOME__ = {home};
            }}
            if (window.__OPENCHAMBER_ELECTRON__ === undefined) {{
                // NOTE: `runtime` is a shell-identity flag, not a technology
                // label. The shared UI treats `runtime === 'electron'` as
                // "compatible desktop shell" (isDesktopShell / isElectronShell
                // in packages/ui/src/lib/desktop.ts). The Tauri shell speaks
                // the same __OPENCHAMBER_DESKTOP__ bridge, so it presents the
                // same identity; anything else silently disables desktop mode
                // (host switcher, tray, boot-outcome flow).
                window.__OPENCHAMBER_ELECTRON__ = {{
                    runtime: 'electron',
                    arch: {arch},
                    trayEnabled: {tray_enabled},
                    version: {version},
                    macosMajor: {macos_major},
                }};
            }}
            if (window.__OPENCHAMBER_PLATFORM__ === undefined) {{
                window.__OPENCHAMBER_PLATFORM__ = {platform};
            }}
        }})();
        "#
    )
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    log::info!("[runtime] quit requested");
    app.exit(0);
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    log::info!("[runtime] restart requested");
    app.restart();
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
        return Err(format!("refusing to open non-http(s) url: {}", url));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reveal_in_file_manager(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct PlatformInfo {
    pub runtime: &'static str,
    pub os: &'static str,
    pub arch: String,
    pub version: String,
}

#[tauri::command]
pub fn platform_info(state: tauri::State<RuntimeConfig>) -> PlatformInfo {
    PlatformInfo {
        runtime: "tauri",
        os: state.platform,
        arch: state.arch.clone(),
        version: state.app_version.clone(),
    }
}

#[tauri::command]
pub fn is_desktop() -> bool {
    true
}

// ── Boot outcome ──

/// Structured boot outcome injected as
/// `window.__OPENCHAMBER_DESKTOP_BOOT_OUTCOME__`.
///
/// Shape must match `DesktopBootOutcome` in
/// `packages/ui/src/lib/desktopBoot.ts`: `{ target, status }` with
/// `target: 'local' | 'remote' | null`. The desktop UI strictly requires a
/// valid outcome before dismissing the splash screen.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BootOutcome {
    pub target: Option<String>,
    pub status: String,
}

impl BootOutcome {
    pub fn local_ok() -> Self {
        Self {
            target: Some("local".to_string()),
            status: "ok".to_string(),
        }
    }

    pub fn local_unreachable() -> Self {
        Self {
            target: Some("local".to_string()),
            status: "unreachable".to_string(),
        }
    }

    /// JS snippet that sets the global the UI polls for.
    /// Serialized with serde_json so quoting is always safe.
    pub fn as_injection_script(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!(
                "window.__OPENCHAMBER_DESKTOP_BOOT_OUTCOME__={};",
                json
            ),
            Err(_) => String::new(),
        }
    }
}

/// Mutable boot-outcome slot, filled once the supervisor settles.
/// The frontend also reads it through `get_boot_outcome` on every page
/// load (covers HMR/dev reloads that wipe the injected global).
pub struct BootOutcomeState(pub tokio::sync::Mutex<Option<BootOutcome>>);

impl BootOutcomeState {
    pub fn new() -> Self {
        Self(tokio::sync::Mutex::new(None))
    }
}

impl Default for BootOutcomeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the last computed boot outcome, or null while the supervisor
/// is still starting. The TS adapter (`applyRuntimeConfig`) applies it to
/// `window.__OPENCHAMBER_DESKTOP_BOOT_OUTCOME__`.
#[tauri::command]
pub async fn get_boot_outcome(
    state: tauri::State<'_, BootOutcomeState>,
) -> Result<Option<BootOutcome>, String> {
    Ok(state.0.lock().await.clone())
}