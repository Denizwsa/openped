//! Settings commands.
//!
//! Mirrors the Electron `mutateSettingsRoot` / `readJsonFile` / `writeJsonFile`
//! helpers. The path is `$OPENCHAMBER_DATA_DIR/settings.json` if set, else
//! `$HOME/.config/openchamber/settings.json`. Writes are atomic (tmp + rename)
//! and serialized through an in-process queue so concurrent settings dialog
//! saves don't trample each other.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::process::SupervisorExt;

const SETTINGS_FILE_NAME: &str = "settings.json";

#[derive(Default)]
pub struct SettingsState {
    queue: Mutex<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEntry {
    pub id: String,
    pub label: String,
    pub url: Option<String>,
    pub api_url: Option<String>,
    pub client_token: Option<String>,
    #[serde(default)]
    pub request_headers: Value,
    pub relay: Option<RelayDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDescriptor {
    pub relay_url: String,
    pub server_id: String,
    pub host_enc_pub_jwk: Value,
}

fn settings_path() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENCHAMBER_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir).join(SETTINGS_FILE_NAME);
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config")
        .join("openchamber")
        .join(SETTINGS_FILE_NAME)
}

fn read_settings_file(path: &Path) -> Value {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Value::Object(Default::default());
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(v) if v.is_object() => v,
        _ => Value::Object(Default::default()),
    }
}

async fn write_settings_file(path: &Path, root: &Value) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    tokio::fs::create_dir_all(parent).await?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), ts));
    let serialized = serde_json::to_string_pretty(root).map_err(std::io::Error::other)?;
    tokio::fs::write(&tmp, serialized).await?;
    if let Err(err) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(err);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Value, String> {
    let _ = app.state::<SettingsState>().inner();
    let path = settings_path();
    Ok(read_settings_file(&path))
}

#[tauri::command]
pub async fn set_settings(app: AppHandle, mut value: Value) -> Result<Value, String> {
    if !value.is_object() {
        return Err("settings root must be an object".to_string());
    }
    let state = app.state::<SettingsState>();
    let _guard = state.queue.lock().await;
    let path = settings_path();
    write_settings_file(&path, &value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(value.clone())
}

#[tauri::command]
pub async fn get_desktop_hosts(app: AppHandle) -> Result<Value, String> {
    let _ = app.supervisor();
    let path = settings_path();
    let root = read_settings_file(&path);
    Ok(root.get("desktopHosts").cloned().unwrap_or_else(|| Value::Array(vec![])))
}

#[tauri::command]
pub async fn set_desktop_hosts(app: AppHandle, hosts: Value) -> Result<(), String> {
    if !hosts.is_array() {
        return Err("hosts must be an array".to_string());
    }
    let state = app.state::<SettingsState>();
    let _guard = state.queue.lock().await;
    let path = settings_path();
    let mut root = read_settings_file(&path);
    if let Some(obj) = root.as_object_mut() {
        obj.insert("desktopHosts".into(), hosts);
    }
    write_settings_file(&path, &root)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}