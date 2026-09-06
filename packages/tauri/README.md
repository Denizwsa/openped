# OpenChamber Tauri — Native Desktop Shell

Tauri 2 shell for OpenChamber. Replaces the upstream Electron shell with a
small Rust binary + the host's native WebView (WebKitGTK on Linux, WKWebView
on macOS). No bundled Chromium.

## What it does

- Owns the native window, tray, notifications, file dialogs, shell-open, and
  the child processes (`opencode serve` + the OpenChamber web server).
- Injects the same `window.__OPENCHAMBER_*` globals the Electron shell used
  to, so `packages/web` and `packages/ui` keep working unchanged.
- Exposes the former `preload.mjs` surface (`window.__OPENCHAMBER_DESKTOP__`)
  as Tauri `invoke` commands in `src-tauri/src/commands/`.

Upstream's `packages/electron` has been deleted. `packages/mobile` (Capacitor)
and `packages/vscode` are untouched — they talk to a running server.

## Prerequisites

### Linux

```bash
sudo pacman -S webkit2gtk-4.1 gtk3 libsoup3 librsvg2-2 \
               base-devel curl wget file openssl appmenu-gtk-module \
               libappindicator-gtk3
# Arch/CachyOS alternative (AUR):
# paru -S webkit2gtk-4.1
```

Rust 1.77+, Node 22+, Bun 1.3+.

### macOS

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# then:
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

## Develop

```bash
bun install
bun run build:web          # one-off Vite build (needed for cargo check)
bun run tauri:dev           # Vite HMR + Tauri window (Linux)
```

`tauri:dev` runs `http://localhost:5173` (Vite) in the Tauri WebView.
The Rust side spawns `opencode serve` (port `OPENCODE_PORT`, default 4096)
and `node packages/web/server/index.js` (port `OPENCHAMBER_PORT`, default
57123) as children and kills them on exit.

| Env | Effect |
|-----|--------|
| `OPENCODE_BINARY` | override opencode binary |
| `OPENCODE_PORT` | opencode server port |
| `OPENCHAMBER_PORT` | openchamber server port |
| `OPENCHAMBER_SERVER_DIR` | use a pre-built `dist-server` dir |
| `OPENCODE_SKIP_START=1` | don't spawn opencode |
| `OPENCHAMBER_SKIP_LOCAL_SERVER=1` | don't spawn openchamber server |

## Build (native bundle)

```bash
# Linux (native arch)
bun run tauri:build
# artifacts: src-tauri/target/release/bundle/{deb,rpm,appimage}

# macOS (from macOS)
bun run tauri:build -- --target aarch64-apple-darwin
bun run tauri:build -- --target x86_64-apple-darwin
# artifacts: target/aarch64-apple-darwin/release/bundle/{dmg,app}
```

Linux builds are natively packaged; cross-arch needs `cargo` cross or a
matching host. macOS builds must run on macOS (no cross from Linux).

## Packaging notes

- Icons live in `src-tauri/icons/` (`icon.png` 1024×1024 RGBA source →
  `node scripts/build-icons.mjs` regenerates `32x32.png`, `128x128@2x.png`,
  `icon.ico`, `icon.icns`).
- Updater is wired to `tauri-plugin-updater` with a GitHub Releases feed.
  Set `pubkey` in `tauri.conf.json` when releases are signed with
  `tauri signer generate`.
- The `frontendDist` is `../../web/dist` (Vite output). Run `bun run build:web`
  before `cargo check --offline`.

## Validate

```bash
cargo check --manifest-path packages/tauri/src-tauri/Cargo.toml --offline
cargo build  --manifest-path packages/tauri/src-tauri/Cargo.toml --offline
bun run build:web
```

`tauri:build` is heavy (Vite + Rust LTO); the three checks above cover the
same surface without bundling.
