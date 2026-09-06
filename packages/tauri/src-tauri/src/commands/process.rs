//! Process commands — wrap the supervisor and expose opencode session APIs
//! proxied through the openchamber web server.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::process::{ServerEndpoints, SupervisorExt};

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionHandle {
    pub id: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
}

#[tauri::command]
pub async fn server_endpoints(app: AppHandle) -> Result<ServerEndpoints, String> {
    let supervisor = app.supervisor();
    Ok(ServerEndpoints {
        openchamber_url: std::env::var("OPENCHAMBER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:57123".to_string()),
        opencode_url: std::env::var("OPENCODE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:4096".to_string()),
        opencode_auth: std::env::var("OPENCODE_SERVER_PASSWORD").ok().filter(|s| !s.is_empty()),
    })
}

#[tauri::command]
pub async fn open_session(
    app: AppHandle,
    parent_id: Option<String>,
    title: Option<String>,
) -> Result<SessionHandle, String> {
    let base = std::env::var("OPENCHAMBER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:57123".to_string());
    let url = format!("{}/session", base.trim_end_matches('/'));
    let body = serde_json::json!({
        "parentID": parent_id,
        "title": title,
    });
    let resp = reqwest_post_json(&url, &body).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("open_session failed: {}", resp.status()));
    }
    let parsed: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(SessionHandle {
        id: parsed
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        title: parsed
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        parent_id: parsed
            .get("parentID")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

#[tauri::command]
pub async fn abort_session(app: AppHandle, id: String) -> Result<bool, String> {
    let base = std::env::var("OPENCHAMBER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:57123".to_string());
    let url = format!("{}/session/{}/abort", base.trim_end_matches('/'), id);
    let resp = reqwest_post_empty(&url).await.map_err(|e| e.to_string())?;
    Ok(resp.status().is_success())
}

#[tauri::command]
pub async fn list_sessions(app: AppHandle) -> Result<Vec<SessionHandle>, String> {
    let base = std::env::var("OPENCHAMBER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:57123".to_string());
    let url = format!("{}/session", base.trim_end_matches('/'));
    let resp = reqwest_get(&url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("list_sessions failed: {}", resp.status()));
    }
    let parsed: Vec<serde_json::Value> = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parsed
        .into_iter()
        .map(|v| SessionHandle {
            id: v.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
            title: v.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()),
            parent_id: v.get("parentID").and_then(|v| v.as_str()).map(|s| s.to_string()),
        })
        .collect())
}

#[tauri::command]
pub async fn pick_opencode_binary(app: AppHandle) -> Result<Option<String>, String> {
    let _ = app.supervisor();
    if let Ok(env) = std::env::var("OPENCODE_BINARY") {
        if std::path::Path::new(&env).is_file() {
            return Ok(Some(env));
        }
    }
    for var in ["OPENCODE_PATH", "OPENCHAMBER_OPENCODE_PATH", "OPENCHAMBER_OPENCODE_BIN"] {
        if let Ok(env) = std::env::var(var) {
            if std::path::Path::new(&env).is_file() {
                return Ok(Some(env));
            }
        }
    }
    Ok(None)
}

async fn reqwest_get(url: &str) -> anyhow::Result<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    Ok(client.get(url).send().await?)
}

async fn reqwest_post_json(url: &str, body: &serde_json::Value) -> anyhow::Result<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    Ok(client.post(url).json(body).send().await?)
}

async fn reqwest_post_empty(url: &str) -> anyhow::Result<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    Ok(client.post(url).send().await?)
}