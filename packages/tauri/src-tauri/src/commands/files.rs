//! File system commands.
//!
//! Mirror the Electron `dialog.showOpenDialog` / `showSaveDialog` plus the
//! `path-open-utils` helpers and `mintOutsideFileGrant`. The OpenChamber web
//! server already mints file grants for inside-workspace paths; outside paths
//! get one approved here through the dialog and persisted in the same shape.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenDialogOptions {
    pub title: Option<String>,
    pub default_path: Option<String>,
    pub directory: Option<bool>,
    #[serde(default)]
    pub filters: Vec<DialogFilter>,
    pub multiple: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenDialogResult {
    pub path: Option<String>,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveDialogOptions {
    pub title: Option<String>,
    pub default_path: Option<String>,
    pub filters: Vec<DialogFilter>,
}

#[tauri::command]
pub async fn open_file_dialog(
    app: AppHandle,
    options: OpenDialogOptions,
) -> Result<OpenDialogResult, String> {
    let directory = options.directory.unwrap_or(false);
    let multiple = options.multiple.unwrap_or(false);
    let (tx, rx) = tokio::sync::oneshot::channel();
    if directory {
        let mut builder = app
            .dialog()
            .file()
            .set_title(options.title.unwrap_or_else(|| "Open".to_string()));
        if let Some(p) = options.default_path.as_ref() {
            builder = builder.set_directory(PathBuf::from(p));
        }
        builder.pick_folder(move |maybe_path| {
            let _ = tx.send(maybe_path);
        });
    } else if multiple {
        let mut builder = app
            .dialog()
            .file()
            .set_title(options.title.unwrap_or_else(|| "Open".to_string()));
        if let Some(p) = options.default_path.as_ref() {
            builder = builder.set_directory(PathBuf::from(p));
        }
        builder.pick_files(move |maybe_paths| {
            let _ = tx.send(maybe_paths.and_then(|v| v.into_iter().next()));
        });
    } else {
        let mut builder = app
            .dialog()
            .file()
            .set_title(options.title.unwrap_or_else(|| "Open".to_string()));
        if let Some(p) = options.default_path.as_ref() {
            builder = builder.set_directory(PathBuf::from(p));
        }
        builder.pick_file(move |maybe_path| {
            let _ = tx.send(maybe_path);
        });
    }
    let result = rx.await.map_err(|e| e.to_string())?;
    let path = result
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().to_string());
    Ok(OpenDialogResult {
        path: path.clone(),
        paths: path.into_iter().collect(),
    })
}

#[tauri::command]
pub async fn save_file_dialog(
    app: AppHandle,
    options: SaveDialogOptions,
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut builder = app
        .dialog()
        .file()
        .set_title(options.title.unwrap_or_else(|| "Save".to_string()));
    if let Some(p) = options.default_path.as_ref() {
        builder = builder.set_file_name(p);
    }
    builder.save_file(|maybe_path| {
        let _ = tx.send(maybe_path);
    });
    let result = rx.await.map_err(|e| e.to_string())?;
    Ok(result
        .and_then(|fp| fp.into_path().ok())
        .map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read_text_file({}): {}", path, e))
}

#[tauri::command]
pub async fn write_text_file(path: String, content: String) -> Result<(), String> {
    tokio::fs::write(&path, content)
        .await
        .map_err(|e| format!("write_text_file({}): {}", path, e))
}

#[tauri::command]
pub async fn path_exists(path: String) -> Result<bool, String> {
    Ok(std::path::Path::new(&path).exists())
}