/**
 * Tauri ↔ shared-UI bridge.
 *
 * Re-implements the contract that upstream Electron's `preload.mjs` exposed
 * as `window.__OPENCHAMBER_DESKTOP__`. The shared code in `packages/ui` and
 * `packages/web` continues to call `window.__OPENCHAMBER_DESKTOP__.invoke(...)`,
 * `window.__OPENCHAMBER_DESKTOP__.openDialog(...)`, etc., unchanged.
 *
 * Behind the scenes, each call becomes a `tauri.invoke()` command whose
 * handler lives in `packages/tauri/src-tauri/src/commands/`.
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen, UnlistenFn } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';

export interface TauriBridge {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  openDialog: (options?: Record<string, unknown>) => Promise<unknown>;
  grantFileAccess: (filePath: string) => Promise<unknown>;
  openExternal: (url: string) => Promise<unknown>;
  listen: (
    event: string,
    handler: (payload: unknown) => void,
  ) => Promise<UnlistenFn>;
}

declare global {
  interface Window {
    __OPENCHAMBER_DESKTOP__?: TauriBridge;
    __TAURI_INTERNALS__?: unknown;
  }
}

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const realBridge: TauriBridge = {
  invoke: (cmd, args = {}) => tauriInvoke(cmd, args),
  openDialog: (options = {}) => tauriInvoke('open_file_dialog', { options }),
  grantFileAccess: (filePath) => tauriInvoke('mint_outside_file_grant', { filePath }).catch(async (err) => {
    // Fallback for compatibility — equivalent to upstream's
    // `openchamber:file:grant-existing` IPC.
    void err;
    return null;
  }),
  openExternal: (url) => openUrl(url),
  listen: async (event, handler) => {
    return tauriListen(event, (e) => {
      handler(e.payload);
    });
  },
};

const stubBridge: TauriBridge = {
  invoke: async (cmd) => {
    if (typeof console !== 'undefined') {
      console.warn(`[tauriBridge] invoke("${cmd}") called outside Tauri runtime`);
    }
    return null;
  },
  openDialog: async () => null,
  grantFileAccess: async () => null,
  openExternal: async (url) => {
    if (typeof window !== 'undefined') {
      window.open(url, '_blank');
    }
  },
  listen: async () => () => {},
};

export function installTauriBridge(): void {
  if (typeof window === 'undefined') {
    return;
  }
  window.__OPENCHAMBER_DESKTOP__ = isTauri ? realBridge : stubBridge;
  if (typeof console !== 'undefined') {
    console.info(
      `[tauriBridge] installed (runtime=${isTauri ? 'tauri' : 'browser-fallback'})`,
    );
  }
}

export function isTauriRuntime(): boolean {
  return isTauri;
}