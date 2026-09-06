//! Application bootstrap.
//!
//! Wires Tauri plugins, manages the supervisor state, and sets up the
//! initial WebView window. The order matters: plugins first, then the
//! supervisor (so its `start()` runs in `setup` before the window is shown),
//! then the commands are registered.
//!
//! Runtime config (API base URL, client token, home dir, macOS hints) is
//! injected into the dev webview via `WebviewWindowBuilder::initialization_script`
//! (matches the Electron `initScript` behavior). In packaged builds the
//! renderer asks for it through `commands::runtime::get_runtime_config`.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::commands;
use crate::commands::runtime::{BootOutcome, BootOutcomeState};
use crate::process::{Supervisor, SupervisorExt};
use crate::runtime_config::RuntimeConfig;

const DEFAULT_OPENCHAMBER_PORT: u16 = 57123;
const DEFAULT_OPENCODE_PORT: u16 = 4096;

pub fn run() {
    let openchamber_port: u16 = std::env::var("OPENCHAMBER_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_OPENCHAMBER_PORT);

    let opencode_port: u16 = std::env::var("OPENCODE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_OPENCODE_PORT);

    let openchamber_url = format!("http://127.0.0.1:{}", openchamber_port);
    let opencode_url = format!("http://127.0.0.1:{}", opencode_port);

    let runtime_config = RuntimeConfig::for_local_origin(openchamber_url.clone(), openchamber_port);

    let mut builder = tauri::Builder::default();

    builder = builder
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(runtime_config.clone())
        .manage(Supervisor::new())
        .manage(BootOutcomeState::new())
        .setup(move |app| {
            let handle = app.handle().clone();

            // Start the opencode CLI and the openchamber web server. They run
            // as children of the Tauri process and are killed at exit.
            // Afterwards compute the boot outcome the desktop UI waits for
            // before dismissing its splash screen, store it for
            // `get_boot_outcome`, and push it into the main window.
            let supervisor = handle.supervisor();
            let openchamber_url_clone = openchamber_url.clone();
            let opencode_url_clone = opencode_url.clone();
            let handle_for_boot = handle.clone();
            tauri::async_runtime::spawn(async move {
                let handle = handle_for_boot;
                let start_result = supervisor
                    .start(openchamber_url_clone.clone(), opencode_url_clone)
                    .await;
                if let Err(err) = &start_result {
                    log::error!("[supervisor] failed to start: {err:#}");
                }

                let outcome = if start_result.is_ok() {
                    BootOutcome::local_ok()
                } else {
                    BootOutcome::local_unreachable()
                };

                {
                    let state = handle.state::<BootOutcomeState>();
                    *state.0.lock().await = Some(outcome.clone());
                }

                let script = outcome.as_injection_script();
                if !script.is_empty() {
                    if let Some(window) = handle.get_webview_window("main") {
                        if let Err(err) = window.eval(&script) {
                            log::warn!("[bootstrap] boot outcome injection failed: {err:#}");
                        } else {
                            log::info!("[bootstrap] boot outcome injected: {:?}", outcome.status);
                        }
                    }
                }
            });

            // Open the dev server URL when developing, the built bundle when
            // packaged. The runtime config injection runs on whichever is
            // loaded.
            let url = if cfg!(debug_assertions) {
                WebviewUrl::App(
                    std::env::var("OPENCHAMBER_DEV_URL")
                        .unwrap_or_else(|_| "http://localhost:5173".to_string())
                        .parse()
                        .expect("OPENCHAMBER_DEV_URL must be a valid URL"),
                )
            } else {
                WebviewUrl::App("index.html".into())
            };

            let window = WebviewWindowBuilder::new(app, "main", url)
                .title("OpenChamber")
                .inner_size(1280.0, 820.0)
                .min_inner_size(800.0, 520.0)
                .resizable(true)
                .initialization_script(&commands::runtime::build_initialization_script(
                    &runtime_config,
                ))
                .build()
                .expect("failed to build main window");

            // Trigger window-state restoration after the window is ready so
            // we honor the persisted bounds/size/maximized/fullscreen from
            // the previous session.
            let handle_for_restore = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if handle_for_restore.get_webview_window("main").is_none() {
                    log::debug!("[bootstrap] main window not ready yet");
                }
            });

            // Deep-link registration: openchamber://<path> or
            // openchamber://<host>/<path> — same scheme the upstream Electron
            // build registers.
            #[cfg(any(target_os = "linux", all(debug_assertions)))]
            {
                if let Err(err) = app.deep_link().register("openchamber") {
                    log::warn!("[bootstrap] deep-link register failed: {err:#}");
                }
            }

            log::info!("[bootstrap] openchamber-tauri ready");
            log::info!("[bootstrap] openchamber server: {}", openchamber_url);
            log::info!("[bootstrap] opencode server:    {}", opencode_url);
            log::info!("[bootstrap] home directory:    {}", runtime_config.home_directory);
            let _ = window;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    log::info!("[bootstrap] main window close requested");
                    let _ = api;
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::runtime::get_runtime_config,
            commands::runtime::get_boot_outcome,
            commands::runtime::quit_app,
            commands::runtime::restart_app,
            commands::runtime::open_external,
            commands::runtime::reveal_in_file_manager,
            commands::runtime::platform_info,
            commands::runtime::is_desktop,
            commands::process::server_endpoints,
            commands::process::open_session,
            commands::process::abort_session,
            commands::process::list_sessions,
            commands::process::pick_opencode_binary,
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::settings::get_desktop_hosts,
            commands::settings::set_desktop_hosts,
            commands::files::open_file_dialog,
            commands::files::save_file_dialog,
            commands::files::read_text_file,
            commands::files::write_text_file,
            commands::files::path_exists,
            commands::notifications::notify,
            commands::notifications::is_supported,
            commands::updates::check_for_update,
            commands::updates::install_update,
            commands::window::new_mini_chat_window,
            commands::window::focus_main_window,
            commands::window::set_tray_visible,
            commands::desktop::desktop_hosts_get,
            commands::desktop::desktop_hosts_set,
            commands::desktop::desktop_local_client_token_get,
            commands::desktop::desktop_host_probe,
            commands::desktop::desktop_open_path,
            commands::desktop::desktop_reveal_path,
            commands::desktop::desktop_restart,
            commands::desktop::desktop_show_app_menu,
            commands::desktop::desktop_tray_update,
            commands::desktop::desktop_check_for_updates,
            commands::desktop::desktop_get_keep_awake,
            commands::desktop::desktop_set_keep_awake,
            commands::desktop::desktop_get_lan_address,
            commands::desktop::desktop_ssh_status,
            commands::desktop::desktop_ssh_connect,
            commands::desktop::desktop_ssh_disconnect,
            commands::desktop::desktop_ssh_logs,
            commands::desktop::desktop_ssh_logs_clear,
            commands::desktop::desktop_browser_capture_page,
            commands::desktop::desktop_browser_clear_data,
            commands::desktop::desktop_browser_set_color_scheme,
            commands::desktop::desktop_dev_tunnel_open,
            commands::desktop::desktop_relay_dev_tunnel_close_all,
            commands::desktop::desktop_new_window_at_url,
            commands::desktop::desktop_new_window_for_host,
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running openchamber-tauri");
}

pub fn _export_app_handle(handle: AppHandle) -> AppHandle {
    handle
}