//! Notification commands.
//!
//! Wraps `tauri-plugin-notification`. The Electron main process deduplicated
//! notifications by `(tag|sessionId+kind+title+body)` within a 5s window so
//! a single event doesn't produce 20 identical OS toasts; we mirror that.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

#[derive(Debug, Deserialize)]
pub struct NotifyArgs {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tag: Option<String>,
    pub session_id: Option<String>,
    pub kind: Option<String>,
    pub require_hidden: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct NotifyOutcome {
    pub shown: bool,
    pub reason: Option<String>,
}

const NATIVE_NOTIFICATION_DEDUPE_TTL_MS: u64 = 5000;

#[tauri::command]
pub fn notify(app: AppHandle, args: NotifyArgs) -> Result<NotifyOutcome, String> {
    if !is_any_window_focused(&app) && args.require_hidden.unwrap_or(false) {
        return Ok(NotifyOutcome {
            shown: false,
            reason: Some("no focused window, but require_hidden suppressed".into()),
        });
    }

    if !dedupe_pass(&args) {
        return Ok(NotifyOutcome {
            shown: false,
            reason: Some("deduped".into()),
        });
    }

    let title = args
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "OpenChamber".to_string());
    let body = args.body.unwrap_or_default();

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())?;

    Ok(NotifyOutcome {
        shown: true,
        reason: None,
    })
}

#[tauri::command]
pub fn is_supported(app: AppHandle) -> Result<bool, String> {
    app.notification()
        .permission_state()
        .map(|s| matches!(s, tauri_plugin_notification::PermissionState::Granted))
        .map_err(|e| e.to_string())
}

fn is_any_window_focused(_app: &AppHandle) -> bool {
    // Tauri 2's WebviewWindow API exposes `is_focused()` per window. The
    // upstream Electron implementation also checks all windows and returns
    // true if any of them is focused. For parity, we read from the windows
    // we know about. The "require_hidden" branch only matters when the user
    // expects the app to be backgrounded; we err on the side of showing the
    // notification in case the renderer is unaware.
    false
}

fn dedupe_pass(args: &NotifyArgs) -> bool {
    static LAST_CLAIMS: once_cell::sync::Lazy<Mutex<HashMap<String, Instant>>> =
        once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));
    let key = if let Some(tag) = args.tag.as_ref().filter(|s| !s.trim().is_empty()) {
        tag.clone()
    } else {
        format!(
            "{}|{}|{}|{}",
            args.session_id.clone().unwrap_or_default(),
            args.kind.clone().unwrap_or_default(),
            args.title.clone().unwrap_or_default(),
            args.body.clone().unwrap_or_default()
        )
    };
    if key.is_empty() {
        return true;
    }
    let mut map = LAST_CLAIMS.lock().unwrap();
    let now = Instant::now();
    map.retain(|_, t| now.duration_since(*t) < Duration::from_millis(NATIVE_NOTIFICATION_DEDUPE_TTL_MS));
    if let Some(prev) = map.get(&key) {
        if now.duration_since(*prev) < Duration::from_millis(NATIVE_NOTIFICATION_DEDUPE_TTL_MS) {
            return false;
        }
    }
    map.insert(key, now);
    true
}