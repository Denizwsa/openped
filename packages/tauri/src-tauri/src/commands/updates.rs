//! Updater commands — wrappers over `tauri-plugin-updater`.
//!
//! Returns a structured `UpdateStatus` so the renderer can drive its update
//! dialog the same way the Electron build did.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Serialize)]
pub struct UpdateStatus {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    match app.updater().map_err(|e| e.to_string())?.check().await {
        Ok(Some(update)) => Ok(UpdateStatus {
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
            error: None,
        }),
        Ok(None) => Ok(UpdateStatus {
            available: false,
            version: None,
            notes: None,
            date: None,
            error: None,
        }),
        Err(err) => Ok(UpdateStatus {
            available: false,
            version: None,
            notes: None,
            date: None,
            error: Some(err.to_string()),
        }),
    }
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<bool, String> {
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let mut installed = false;
    update
        .download_and_install(
            |chunk, total| {
                log::debug!("[updater] {:?}/{:?} bytes", chunk, total);
            },
            || {
                installed = true;
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(installed)
}