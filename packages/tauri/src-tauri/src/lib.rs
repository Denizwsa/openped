//! OpenChamber Tauri shell.
//!
//! The shell owns the native window, the child processes (opencode CLI and
//! openchamber web server), and the privileged command surface exposed to the
//! shared UI as `window.__OPENCHAMBER_DESKTOP__`.
//!
//! Layering:
//!   - [`bootstrap`]: app setup, plugin registration, state, lifecycle hooks.
//!   - [`process`]: child process supervisor (opencode + openchamber server).
//!   - [`commands`]: thin `#[tauri::command]` surface mirroring the upstream
//!     `preload.mjs` `__OPENCHAMBER_DESKTOP__` contract.
//!   - [`runtime_config`]: globals (API base URL, client token, runtime
//!     headers) injected into the renderer the same way the Electron main
//!     process injected them via `<script>` injection on `index.html`.

mod bootstrap;
mod commands;
mod process;
mod runtime_config;

pub fn run() {
    bootstrap::run();
}