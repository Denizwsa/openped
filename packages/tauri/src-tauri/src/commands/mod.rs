//! Thin command modules mapped 1:1 onto `preload.mjs`'s
//! `window.__OPENCHAMBER_DESKTOP__` surface. Each submodule owns its
//! privilege boundary and keeps the body small; heavy logic lives in
//! owning modules (`process`, `runtime_config`, settings layer, etc.).
//!
//! Adding a new native capability follows the same flow as Electron's IPC:
//! 1.  Add the handler here.
//! 2.  Register it in `bootstrap::run`'s `invoke_handler`.
//! 3.  Add a frontend wrapper in `packages/web/src/runtime/tauriBridge.ts`
//!     that calls `invoke('name', args)` from the shared UI.
//! 4.  Open the corresponding capability in `capabilities/default.json` if
//!     a Tauri plugin permission is involved.

pub mod files;
pub mod notifications;
pub mod process;
pub mod runtime;
pub mod settings;
pub mod updates;
pub mod window;