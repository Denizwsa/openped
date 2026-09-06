/**
 * Tauri runtime config adapter.
 *
 * The upstream Electron main process injected `__OPENCHAMBER_LOCAL_ORIGIN__`
 * and friends into the served `index.html`. In dev mode the Tauri shell does
 * the same through `WebviewWindowBuilder::initialization_script`. In packaged
 * builds the static HTML can't be mutated, so the renderer reads the same
 * shape through `invoke('get_runtime_config')` and applies it to the same
 * globals.
 */

import { invoke } from '@tauri-apps/api/core';

export interface RuntimeConfig {
  local_origin: string;
  api_base_url: string;
  client_token: string;
  runtime_headers: Record<string, string>;
  relay_host_id: string;
  home_directory: string;
  macos_major: number | null;
  platform: string;
  arch: string;
  tray_enabled: boolean;
  app_version: string;
}

export async function applyRuntimeConfig(): Promise<RuntimeConfig | null> {
  if (typeof window === 'undefined') return null;

  const initAlreadyApplied =
    typeof window.__OPENCHAMBER_ELECTRON__ !== 'undefined' &&
    window.__OPENCHAMBER_ELECTRON__.runtime === 'tauri';
  if (initAlreadyApplied) {
    // Initialization script already ran (dev reload/HMR). The boot outcome
    // global may still have been wiped by the reload, so always re-apply it.
    await applyBootOutcome();
    return null;
  }

  try {
    const cfg = await invoke<RuntimeConfig>('get_runtime_config');
    if (!cfg) return null;
    if (!window.__OPENCHAMBER_LOCAL_ORIGIN__) {
      window.__OPENCHAMBER_LOCAL_ORIGIN__ = cfg.local_origin;
    }
    if (!window.__OPENCHAMBER_API_BASE_URL__) {
      window.__OPENCHAMBER_API_BASE_URL__ = cfg.api_base_url;
    }
    if (cfg.client_token && !window.__OPENCHAMBER_CLIENT_TOKEN__) {
      window.__OPENCHAMBER_CLIENT_TOKEN__ = cfg.client_token;
    }
    if (!window.__OPENCHAMBER_RUNTIME_HEADERS__) {
      window.__OPENCHAMBER_RUNTIME_HEADERS__ = cfg.runtime_headers;
    }
    if (cfg.relay_host_id && !window.__OPENCHAMBER_RELAY_HOST_ID__) {
      window.__OPENCHAMBER_RELAY_HOST_ID__ = cfg.relay_host_id;
    }
    if (cfg.home_directory && !window.__OPENCHAMBER_HOME__) {
      window.__OPENCHAMBER_HOME__ = cfg.home_directory;
    }
    if (!window.__OPENCHAMBER_ELECTRON__) {
      // Same identity flag as the Rust initialization script: 'electron'
      // means "compatible desktop shell" to the shared UI.
      window.__OPENCHAMBER_ELECTRON__ = {
        runtime: 'electron',
        arch: cfg.arch,
        trayEnabled: cfg.tray_enabled,
        version: cfg.app_version,
        macosMajor: cfg.macos_major,
      };
    }
    if (!window.__OPENCHAMBER_PLATFORM__) {
      window.__OPENCHAMBER_PLATFORM__ = cfg.platform;
    }
    await applyBootOutcome();
    return cfg;
  } catch (err) {
    console.warn('[runtimeConfig] get_runtime_config failed', err);
    return null;
  }
}

/**
 * Pull the supervisor's boot outcome and expose it as
 * `window.__OPENCHAMBER_DESKTOP_BOOT_OUTCOME__`.
 *
 * The desktop UI polls this global and refuses to dismiss its splash
 * screen until a valid outcome appears. The Rust backend pushes it via
 * `WebviewWindow::eval` once the supervisor settles; this fetch covers
 * reloads (HMR, manual refresh) that wipe the injected global.
 */
export async function applyBootOutcome(): Promise<void> {
  if (typeof window === 'undefined') return;
  if (window.__OPENCHAMBER_DESKTOP_BOOT_OUTCOME__ !== undefined) return;
  try {
    const outcome = await invoke<{
      target: string | null;
      status: string;
    } | null>('get_boot_outcome');
    if (outcome && typeof outcome.status === 'string') {
      window.__OPENCHAMBER_DESKTOP_BOOT_OUTCOME__ = outcome;
    }
  } catch {
    // Supervisor still starting — the Rust-side eval will set it, and the
    // App's own poller keeps waiting.
  }
}

declare global {
  interface Window {
    __OPENCHAMBER_LOCAL_ORIGIN__?: string;
    __OPENCHAMBER_API_BASE_URL__?: string;
    __OPENCHAMBER_CLIENT_TOKEN__?: string;
    __OPENCHAMBER_RUNTIME_HEADERS__?: Record<string, string>;
    __OPENCHAMBER_RELAY_HOST_ID__?: string;
    __OPENCHAMBER_HOME__?: string;
    __OPENCHAMBER_ELECTRON__?: {
      runtime: string;
      arch: string;
      trayEnabled: boolean;
      version: string;
      macosMajor: number | null;
    };
    __OPENCHAMBER_PLATFORM__?: string;
    __OPENCHAMBER_DESKTOP_BOOT_OUTCOME__?: {
      target: string | null;
      status: string;
    };
  }
}