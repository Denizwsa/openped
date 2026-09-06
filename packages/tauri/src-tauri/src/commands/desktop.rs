//! `desktop_*` command surface.
//!
//! These are the EXACT command names the shared UI invokes through
//! `window.__OPENCHAMBER_DESKTOP__.invoke('desktop_...')` (see
//! `packages/ui/src/lib/desktop.ts`, `desktopHosts.ts`, `desktopSsh.ts`).
//! The upstream Electron shell handled them in `ipcMain.handle(
//! 'openchamber:invoke')`; here each is a `#[tauri::command]`.
//!
//! Rules:
//! - Names are a frozen contract — never rename.
//! - Boot-path commands (`desktop_hosts_get`, `desktop_host_probe`,
//!   `desktop_local_client_token_get`) must NEVER throw for a healthy local
//!   setup; return degraded shapes instead.
//! - Feature stubs (SSH, browser panel, tunnels) return the shapes callers
//!   already handle (`[]`, `null`) so those panels degrade gracefully until
//!   natively implemented.

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

use super::settings::{read_settings_file, settings_path, write_settings_file};

// ── hosts ──

/// Full hosts config shape read by `desktopHostsGet()`:
/// `{ hosts, defaultHostId, initialHostChoiceCompleted, localOrigin?,
///    localClientToken? }` (snake_case variants accepted client-side).
#[tauri::command]
pub async fn desktop_hosts_get(app: AppHandle) -> Result<Value, String> {
    let _ = app;
    let root = read_settings_file(&settings_path());
    let hosts = root
        .get("desktopHosts")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let default_host_id = root
        .get("desktopDefaultHostId")
        .and_then(|v| v.as_str())
        .unwrap_or("local");
    let initial = root
        .get("desktopInitialHostChoiceCompleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let local_client_token = root
        .get("desktopLocalClientToken")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    Ok(serde_json::json!({
        "hosts": hosts,
        "defaultHostId": default_host_id,
        "initialHostChoiceCompleted": initial,
        "localClientToken": local_client_token,
    }))
}

/// Accepts `{ input: { hosts, defaultHostId, initialHostChoiceCompleted,
/// localClientToken? } }` (see `desktopHostsSet`).
#[tauri::command]
pub async fn desktop_hosts_set(app: AppHandle, input: Value) -> Result<(), String> {
    let _ = app;
    let path = settings_path();
    let mut root = read_settings_file(&path);
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings root must be an object".to_string())?;
    if let Some(hosts) = input.get("hosts") {
        obj.insert("desktopHosts".into(), hosts.clone());
    }
    if let Some(id) = input.get("defaultHostId").and_then(|v| v.as_str()) {
        obj.insert(
            "desktopDefaultHostId".into(),
            Value::String(id.to_string()),
        );
    }
    if let Some(done) = input.get("initialHostChoiceCompleted").and_then(|v| v.as_bool()) {
        obj.insert("desktopInitialHostChoiceCompleted".into(), Value::Bool(done));
    }
    if let Some(token) = input.get("localClientToken").and_then(|v| v.as_str()) {
        if token.is_empty() {
            obj.remove("desktopLocalClientToken");
        } else {
            obj.insert(
                "desktopLocalClientToken".into(),
                Value::String(token.to_string()),
            );
        }
    }
    write_settings_file(&path, &root)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn desktop_local_client_token_get(app: AppHandle) -> Result<String, String> {
    let _ = app;
    let root = read_settings_file(&settings_path());
    Ok(root
        .get("desktopLocalClientToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}

// ── host probe ──

/// Probe a host URL like the Electron `probeHostWithTimeout` did:
/// identity-gate `/health` when `expectedServerId` is set, then check
/// `/api/version` (+auth) and classify.
/// Returns `{ status: ok|auth|update-recommended|incompatible|wrong-service|
/// unreachable, latencyMs }`.
#[tauri::command]
pub async fn desktop_host_probe(
    app: AppHandle,
    url: String,
    client_token: Option<String>,
    request_headers: Option<Value>,
    expected_server_id: Option<String>,
) -> Result<Value, String> {
    let _ = app;
    let started = std::time::Instant::now();
    let latency_ms = || started.elapsed().as_millis() as u64;
    let bad = |status: &str| {
        serde_json::json!({ "status": status, "latencyMs": latency_ms() })
    };

    let base = url.trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    if let Some(Value::Object(map)) = request_headers {
        for (k, v) in map {
            if let (Ok(name), Some(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                v.as_str(),
            ) {
                if let Ok(value) = reqwest::header::HeaderValue::from_str(val) {
                    headers.insert(name, value);
                }
            }
        }
    }
    if let Some(token) = client_token.filter(|t| !t.trim().is_empty()) {
        if let Ok(value) =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.trim()))
        {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }

    // Identity gate for learned addresses: verify the unauthenticated
    // /health identity before the token-carrying fetch.
    if let Some(expected) = expected_server_id.filter(|s| !s.trim().is_empty()) {
        match client
            .get(format!("{}/health", base))
            .timeout(std::time::Duration::from_secs(4))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(payload) = resp.json::<Value>().await {
                    let reported = payload
                        .get("serverId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    if !reported.is_empty() && reported != expected.trim() {
                        return Ok(bad("wrong-service"));
                    }
                }
            }
            _ => {}
        }
    }

    let version_resp = match client
        .get(format!("{}/api/version", base))
        .headers(headers.clone())
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return Ok(bad("unreachable")),
    };
    let status = version_resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(bad("auth"));
    }
    if !status.is_success() {
        return Ok(bad("unreachable"));
    }
    let payload: Value = version_resp.json().await.unwrap_or(Value::Null);
    Ok(serde_json::json!({
        "status": classify_version_payload(&payload),
        "latencyMs": latency_ms(),
    }))
}

fn classify_version_payload(payload: &Value) -> &'static str {
    let compat = match payload.get("compatibility") {
        Some(c) => c,
        None => return "wrong-service",
    };
    if payload.get("status").and_then(|v| v.as_str()) != Some("ok") {
        return "wrong-service";
    }
    let caps = compat
        .get("capabilities")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let has_runtime = caps.iter().any(|v| v.as_str() == Some("api.runtime-url.v1"));
    if !has_runtime {
        return "incompatible";
    }
    let api_version = compat.get("apiVersion").and_then(|v| v.as_u64()).unwrap_or(0);
    let min_client = compat
        .get("minClientApiVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if api_version != 1 || min_client > 1 {
        return "update-recommended";
    }
    "ok"
}

// ── paths ──

#[tauri::command]
pub async fn desktop_open_path(app: AppHandle, path: String) -> Result<bool, String> {
    Ok(app.opener().open_path(path, None::<&str>).is_ok())
}

#[tauri::command]
pub async fn desktop_reveal_path(app: AppHandle, path: String) -> Result<bool, String> {
    Ok(app.opener().reveal_item_in_dir(path).is_ok())
}

// ── app lifecycle ──

#[tauri::command]
pub fn desktop_restart(app: AppHandle) {
    app.restart();
}

#[tauri::command]
pub fn desktop_show_app_menu() -> Result<(), String> {
    // No native application menu yet; the in-app menus cover this.
    Ok(())
}

#[tauri::command]
pub fn desktop_tray_update(_snapshot: Value) -> Result<(), String> {
    // Tray icon wiring lands with the tray controller; accept-and-ignore
    // so the sync hook never throws.
    Ok(())
}

// ── updates ──

/// Shape read by `checkDesktopUpdates()` (`UpdateInfo` in desktop.ts).
#[tauri::command]
pub async fn desktop_check_for_updates(app: AppHandle) -> Result<Value, String> {
    let current_version = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(serde_json::json!({
            "available": true,
            "version": update.version,
            "currentVersion": current_version,
            "body": update.body,
            "date": update.date.map(|d| d.to_string()),
        })),
        Ok(None) => Ok(serde_json::json!({
            "available": false,
            "currentVersion": current_version,
        })),
        Err(err) => Ok(serde_json::json!({
            "available": false,
            "currentVersion": current_version,
            "error": err.to_string(),
        })),
    }
}

// ── power / network ──

#[tauri::command]
pub fn desktop_get_keep_awake() -> Result<Value, String> {
    Ok(serde_json::json!({ "supported": false, "enabled": false, "active": false }))
}

#[tauri::command]
pub fn desktop_set_keep_awake(_enabled: bool) -> Result<Value, String> {
    Ok(serde_json::json!({ "supported": false, "enabled": false, "active": false }))
}

#[tauri::command]
pub fn desktop_get_lan_address() -> Result<Option<String>, String> {
    // First non-loopback IPv4 from `hostname -I` (Linux/macOS).
    let out = std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(None);
    }
    let first = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(|s| s.to_string())
        .find(|ip| !ip.starts_with("127.") && ip.contains('.'));
    Ok(first)
}

// ── ssh (stubs: panel degrades gracefully) ──

#[tauri::command]
pub fn desktop_ssh_status(_id: Option<String>) -> Result<Value, String> {
    Ok(Value::Array(vec![]))
}

#[tauri::command]
pub fn desktop_ssh_connect(_id: String) -> Result<(), String> {
    Err("SSH is not implemented in the Tauri shell yet".to_string())
}

#[tauri::command]
pub fn desktop_ssh_disconnect(_id: String) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn desktop_ssh_logs(_id: String, _limit: Option<u64>) -> Result<Value, String> {
    Ok(Value::Array(vec![]))
}

#[tauri::command]
pub fn desktop_ssh_logs_clear(_id: String) -> Result<(), String> {
    Ok(())
}

// ── browser panel (stubs: Electron-only surface) ──

#[tauri::command]
pub fn desktop_browser_capture_page(_web_contents_id: Option<f64>) -> Result<Value, String> {
    Ok(Value::Null)
}

#[tauri::command]
pub fn desktop_browser_clear_data() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn desktop_browser_set_color_scheme(
    _web_contents_id: Option<f64>,
    _scheme: Option<String>,
) -> Result<(), String> {
    Ok(())
}

// ── dev tunnels (stubs) ──

#[tauri::command]
pub fn desktop_dev_tunnel_open(_args: Value) -> Result<Value, String> {
    // Null signals DevTunnelUnavailableError client-side (handled).
    Ok(Value::Null)
}

#[tauri::command]
pub fn desktop_relay_dev_tunnel_close_all() -> Result<(), String> {
    Ok(())
}

// ── extra windows ──

fn dev_or_prod_url(app: &AppHandle, page: &str) -> String {
    if cfg!(debug_assertions) {
        std::env::var("OPENCHAMBER_DEV_URL")
            .unwrap_or_else(|_| "http://localhost:5173".to_string())
    } else {
        let _ = (app, page);
        page.to_string()
    }
}

#[tauri::command]
pub async fn desktop_new_window_at_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let label = format!("extra-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0));
    let parsed: url::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let target = WebviewUrl::External(parsed);
    WebviewWindowBuilder::new(&app, &label, target)
        .title("OpenChamber")
        .inner_size(1100.0, 750.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn desktop_new_window_for_host(app: AppHandle, host_id: String) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    // Relay-capable hosts boot the same local UI and pick transport
    // themselves; pass the host id so the new window restores it.
    let base = dev_or_prod_url(&app, "index.html");
    let sep = if base.contains('?') { '&' } else { '?' };
    let parsed: url::Url = format!("{}{}relayHostId={}", base, sep, host_id)
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;
    let target = WebviewUrl::External(parsed);
    let label = format!("host-{}-{}", host_id, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0));
    WebviewWindowBuilder::new(&app, &label, target)
        .title("OpenChamber")
        .inner_size(1100.0, 750.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
