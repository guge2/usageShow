# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Windows system-tray utility (Tauri v2 + React 19 + TypeScript frontend, Rust backend) that polls usage/quota data from locally installed AI coding tools—Claude Code, Codex CLI, Cursor, Amp, Factory Droid, AGY, and Grok—and displays it in a small popup anchored to the system tray.

## Common Commands

```bash
npm run dev       # Start Vite dev server only (frontend only, no Tauri shell)
npm run tauri dev # Full app: compile Rust backend, launch tray + popup
npm run build     # tsc type check + vite build (frontend only, output in dist/)
npm run tauri build   # Build production installer
```

Rust side (under `src-tauri/`):

```bash
cargo check
cargo test --lib             # unit tests for parsing, proxy, and registry logic
cargo run --example probe    # run every adapter for real and print the result
```

`examples/probe.rs` is the fastest way to diagnose a provider: it exercises the
real credential files and the real network path without launching the GUI, and
prints status/metrics only (never tokens). It accepts `--no-proxy` and
`--proxy <addr>` to test connectivity assumptions.

There is no lint/format tooling configured (no eslint/prettier). The frontend
has no test framework; Rust logic is covered by `cargo test --lib`.

## Architecture

**Two independent frontend entry points**, each with its own Vite/React root, sharing `App.css`:
- `index.html` / `src/main.tsx` → `App.tsx`—tray popup; renders a `ProviderCard` for each enabled provider.
- `settings.html` / `src/settings-main.tsx` → `Settings.tsx`—refresh interval, provider toggles, proxy mode, launch-at-startup, and the updater.

The frontend talks to the backend only through `src/api.ts`: `get_usage`,
`refresh_usage`, `get_settings`, `save_settings`, `open_settings_window`,
`list_providers`, `get_proxy_status`. Live data arrives via the `usage-updated`
event, not frontend polling.

### Backend layout (`src-tauri/src/`)

Each file owns exactly one concern; `lib.rs` is assembly only.

| File | Responsibility |
|---|---|
| `lib.rs` | Builds the Tauri app and wires the pieces together |
| `models.rs` | Shared types (`UsageSnapshot`, `UsageMetric`, `AppSettings`, `ProxyMode`) |
| `config.rs` | Loading/saving `settings.json` in the app-config dir |
| `state.rs` | `AppState` plus `refresh()`—the single path a refresh takes |
| `commands.rs` | All `#[tauri::command]` functions |
| `window.rs` | Window creation, tray-anchored placement, show/hide |
| `tray.rs` | Tray icon, menu, global shortcut |
| `scheduler.rs` | Background refresh loop |
| `adapters/` | One module per provider, plus `mod.rs`, `http.rs`, `creds.rs` |

### Adding a provider

1. Write `adapters/<name>.rs` exposing `pub async fn fetch(ctx: FetchCtx) -> UsageSnapshot`.
2. Add one line to the `providers![...]` list in `adapters/mod.rs`.

That is all. `ALL_PROVIDERS` no longer exists in either language—the frontend
fetches the provider list from the backend at runtime via `list_providers`.

### Adapters

`adapters/mod.rs::fetch_enabled()` runs **only the enabled** providers
concurrently via `JoinSet`, preserving registry order. Each adapter is fully
isolated—one failing or panicking never affects the others.

An adapter locates its own credentials (via `creds::home_path`, never hardcoded
paths), reads/decrypts/queries its source (JSON files, Cursor's SQLite
`state.vscdb`, or an authenticated HTTP request), and returns:

- `UsageSnapshot::ok` — fresh data
- `not_connected` — not installed / never logged in
- `error` — credentials found but the request failed
- `no_quota_api` — signed in and healthy, but this account tier exposes no
  usage figures (see AGY below). Deliberately *not* an error; retrying cannot help.

Adapters must use `ctx.client` (never build their own) so the proxy setting
applies uniformly, and should report network failures through
`super::describe_error()` so timeouts point the user at the proxy setting.

### Networking and the proxy layer

`adapters/http.rs` exists because `reqwest` honours `HTTPS_PROXY`/`HTTP_PROXY`
but **does not read the Windows system proxy** from the registry. Several of
these APIs (Anthropic, ChatGPT, Google, Grok) are commonly reachable only
through a local proxy, so without this the app fails while the user's browser
and the vendor CLIs work fine.

`ProxyMode::Auto` (the default) checks the proxy environment variables first,
then `HKCU\...\Internet Settings`. `Off` forces a direct connection; `Manual`
uses the address from Settings. The resolved source is surfaced in the Settings
window via `get_proxy_status`.

### Shared types

`UsageSnapshot`, `UsageMetric`, `AppSettings`, `ProxyMode` are manually kept in
sync between `models.rs` and `types.ts`; update both when changing structures.
`AppSettings` fields use `#[serde(default)]` so configs written by older
versions keep loading.

## Provider notes

**AGY** authenticates via Google OAuth against the internal Code Assist API. Its
quota endpoints (`retrieveUserQuotaSummary`/`retrieveUserQuota`) return
`403 PERMISSION_DENIED` on the **Google AI Plus** tier—by design, per Google's
own upgrade copy ("Google AI Plus users receive the minimum base limits on
Antigravity"). That 403 is reported as `no_quota_api` with the plan name, not as
an error. Only `loadCodeAssist` works on every tier; it is the source of the
plan name. The only credential AGY leaves on disk is
`~/.gemini/oauth_creds.json` (issued to the Gemini CLI OAuth client); the
running CLI refreshes its token in memory, so the disk copy is usually stale and
must be refreshed via its `refresh_token`.

**Grok**'s billing response is gRPC-web framed protobuf, scanned shallowly by
field path. `parse_billing_response` takes `now` as a parameter rather than
reading the clock, so captured responses stay replayable in tests instead of
rotting once their reset time slips into the past.

### Known Cross-Platform Gaps

The project is developed and distributed for Windows; adapter path resolution is
otherwise platform-agnostic. Two known issues would break on macOS/Linux:
- `adapters/amp.rs` hardcodes `amp.exe` when probing `~/.amp/bin`; on non-Windows the binary is `amp`.
- `adapters/claude.rs` only reads `~/.claude/.credentials.json`; on macOS Claude Code stores OAuth credentials in the Keychain, so that file often does not exist even when logged in.
