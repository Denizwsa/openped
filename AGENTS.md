# OpenChamber Tauri Fork

This fork replaces the upstream Electron desktop shell with a Tauri 2 native
shell. The OpenChamber web UI (`packages/web`) and shared UI (`packages/ui`)
remain unchanged; they are bundled into the Tauri shell as a built Vite app and
served from the local Tauri webview.

## Why

- Electron requires shipping Chromium with every install (~150MB+). Tauri uses
  the host's native WebView (WebKitGTK on Linux, WKWebView on macOS) and ships
  a small Rust binary plus the Vite-built JS bundle (~5–15MB total).
- Native menus, tray, notifications, file pickers, and dialogs are wired
  through Tauri's Rust APIs instead of Electron's main-process modules.
- Tauri commands expose the same surface the upstream `preload.mjs` exposed as
  `window.__OPENCHAMBER_DESKTOP__`, so the shared UI doesn't need to be forked
  alongside the shell.

## Runtime Boundaries

- `packages/ui`: shared React UI, state, sync, and runtime contracts.
- `packages/web`: web surfaces, OpenChamber server, managed/external OpenCode lifecycle, CLI.
- `packages/tauri`: native desktop shell and privileged Tauri boundary.
- `packages/vscode`: extension host, webview, and runtime bridge.
- `packages/mobile`: Capacitor iOS/Android shell.

`packages/tauri` is the only shell package this fork ships. The upstream
`packages/electron` has been deleted. `packages/mobile` continues to talk to a
running server (mobile is a client, not a shell host).

## Always-On Constraints

- Do not modify `../opencode`; it is a separate repository.
- Do not run git or GitHub commands unless the user explicitly asks.
- Do not add dependencies unless explicitly requested.
- Never add or log secrets, bearer tokens, pairing credentials, or sensitive user data.
- Keep changes minimal and preserve unrelated worktree changes.
- Release notes are the maintainer's release-time work.
- Enforce security and correctness in core/runtime logic, not only UI visibility or prompts.
- Keep entrypoints and bridges thin; place domain logic in focused owning modules.
- Update owning documentation when module ownership, contracts, or invariants change.

## Tauri-Specific Rules

- The native shell owns `window`, `menu`, `tray`, `notification`, `dialog`,
,
  `fs` access grants, deep links, auto-update, and child process (opencode
  CLI + openchamber web server) lifecycle.
- Shared UI must not import `tauri` packages directly. Tauri APIs are reached
  through the adapter in `packages/web/src/runtime/tauriBridge.ts`, exposed
  as `window.__OPENCHAMBER_DESKTOP__`.
- `Cargo.toml` is the source of truth for Rust dependencies; Tauri commands
  live under `packages/tauri/src/commands/`.
- Packaged builds must include `packages/web`'s built `dist/` and the pinned
  `opencode` CLI binary. The Tauri bundler is configured to stage both.
- Linux requires webkit2gtk-4.1, gtk+-3.0, libsoup-3.0, librsvg2-2. macOS
  requires Xcode command-line tools. Windows is out of scope for this fork.

## Validation

- `bun run type-check:tauri` for Rust + frontend type-check.
- `bun run tauri:dev` for live development against a real opencode CLI.
- `bun run tauri:build` for native .deb / .AppImage / .dmg artifacts.
- The shared UI test suite (`bun run --cwd packages/web test`) runs unchanged.