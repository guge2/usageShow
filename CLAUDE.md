# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Windows system-tray utility (Tauri v2 + React 19 + TypeScript frontend, Rust backend) that polls usage/quota data from locally installed AI coding tools—Claude Code, Codex CLI, Cursor, Amp, and Factory Droid—and displays it in a small popup anchored to the system tray.

## Common Commands

```bash
npm run dev       # Start Vite dev server only (frontend only, no Tauri shell)
npm run tauri dev # Full app: compile Rust backend, launch tray + popup
npm run build     # tsc type check + vite build (frontend only, output in dist/)
npm run tauri build   # Build production installer
```

Rust side (under `src-tauri/`): use standard `cargo check` / `cargo build`.

The project has no lint or test scripts configured (no eslint/prettier setup, no test framework).

Headless diagnostic mode—skips GUI/tray, fetches all providers once, and prints raw JSON snapshots so you can verify adapter output against real local credentials:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- --dump-usage
```

To bypass the "show on tray click / hide on blur" behavior described below during debugging, set `USAGESHOW_FORCE_SHOW=1` before starting `tauri dev` to force the main window to stay visible.

## Architecture

**Two independent frontend entry points**, each with its own Vite/React root, sharing `App.css`:
- `index.html` / `src/main.tsx` → `App.tsx`—tray popup; renders a `ProviderCard` for each enabled provider.
- `settings.html` / `src/settings-main.tsx` → `Settings.tsx`—refresh interval, provider enable checkboxes, and launch-at-startup toggle.

The frontend communicates with Tauri commands registered in `src-tauri/src/lib.rs` only through `src/api.ts` (a wrapper around `invoke`/`listen`): `get_usage`, `refresh_usage`, `get_settings`, `save_settings`, `open_settings_window`. Live data updates are pushed via the `usage-updated` event, not frontend polling.

**Window lifecycle (`lib.rs`)**: The main window is hidden by default (`visible: false` in `tauri.conf.json`). Clicking the tray icon or choosing "Open panel" from the menu shows it via `toggle_window`/`position_window_near_tray` and positions it next to the tray; it auto-hides on blur. `spawn_scheduler` runs a background tokio loop that re-fetches all providers on `settings.refresh_interval_secs`; settings changes wake the loop early via `Notify`. Settings are persisted as JSON in Tauri's app-config directory.

**Adapter pattern (`src-tauri/src/adapters/`)**: `mod.rs::fetch_all()` runs all five provider adapters concurrently with `tokio::join!`. Each adapter is fully self-contained and isolated—one failure does not affect the others. Each adapter exposes only `pub async fn fetch() -> UsageSnapshot` and independently:
- Locates its own credential/executable paths (using `dirs::home_dir()` / `dirs::config_dir()`, never hardcoded paths)
- Reads/decrypts/queries its data source (JSON files, Cursor's SQLite `state.vscdb`, or a single authenticated HTTP request)
- Returns `UsageSnapshot::ok/not_connected/error` (defined in `models.rs`)—`not_connected` means "not installed / never logged in"; `error` means "credentials found but request failed"

To add a provider: create `adapters/<name>.rs` implementing the same `fetch()` contract, register it in `adapters/mod.rs`, and add its id/label to `ALL_PROVIDERS` in both `models.rs` (Rust) and `types.ts` (frontend).

**Shared types** (`UsageSnapshot`, `UsageMetric`, `AppSettings`) are manually kept in sync between `models.rs` and `types.ts`; update both when changing structures.

### Known Cross-Platform Gaps

The project is currently developed and distributed for Windows, but adapter path resolution is written to be platform-agnostic. If you later run on macOS/Linux, two known issues will break:
- `adapters/amp.rs` hardcodes the binary name as `amp.exe` when probing `~/.amp/bin`; on non-Windows the binary is `amp` without an extension.
- `adapters/claude.rs` only reads `~/.claude/.credentials.json`; on macOS Claude Code stores OAuth credentials in the system Keychain by default, so this file often does not exist even when logged in.
