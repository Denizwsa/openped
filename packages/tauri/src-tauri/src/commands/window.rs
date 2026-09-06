//! Window commands — multi-window and tray control.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

#[tauri::command]
pub async fn new_mini_chat_window(app: AppHandle) -> Result<(), String> {
    if app.get_webview_window("mini-chat").is_some() {
        if let Some(window) = app.get_webview_window("mini-chat") {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let url = if cfg!(debug_assertions) {
        WebviewUrl::App(
            std::env::var("OPENCHAMBER_DEV_URL")
                .unwrap_or_else(|_| "http://localhost:5173".to_string())
                .parse()
                .expect("OPENCHAMBER_DEV_URL must be a valid URL"),
        )
    } else {
        WebviewUrl::App("mini-chat.html".into())
    };
    WebviewWindowBuilder::new(&app, "mini-chat", url)
        .title("Mini Chat")
        .inner_size(520.0, 760.0)
        .min_inner_size(360.0, 480.0)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn focus_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        window.unminimize().ok();
    }
    Ok(())
}

#[tauri::command]
pub fn set_tray_visible(_visible: bool) -> Result<(), String> {
    // Tray icon wiring is added in a follow-up step. The upstream feature
    // toggles a system-tray entry per the `desktopMinimizeToTrayEnabled`
    // setting; we expose the same command so the renderer can call it
    // without platform branching.
    Ok(())
}

#[allow(dead_code)]
pub fn restore_window_state(app: &AppHandle) {
    let _ = app.save_window_state(StateFlags::all());
}