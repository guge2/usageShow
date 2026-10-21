# AGENTS.md

This file provides guidance to Codex and other coding agents when working with code in this repository. Keep it in sync with `CLAUDE.md`.

## Project Overview

A Windows system-tray utility (Tauri v2 + React 19 + TypeScript frontend, Rust backend) that polls usage/quota data from locally installed AI coding tools—Claude Code, Codex CLI, Cursor, Amp, Factory Droid, AGY, and Grok—and displays it in a small popup anchored to the system tray.

## Common Commands

```bash
npm run dev       # Start Vite dev server only (frontend only, no Tauri shell)
npm run tauri dev # Full app: compile Rust backend, launch tray + popup
npm run build     # tsc type check + vite build (frontend only, output in dist/)
npm run tauri build   # Build production installer
.\build.ps1           # Windows: build only the optimized release exe (no installer)
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

## Release Workflow

- After normal code changes, use `.\build.ps1`. It enables Cargo incremental compilation, runs `tauri build --no-bundle`, and produces only `src-tauri/target/release/tauri-app.exe`.
- Keep `src-tauri/target/` between builds so Cargo can reuse compiled dependencies. Clean it only when diagnosing stale build output.
- Release builds already exclude Rust debug information. Do not publish `.pdb` files.
- Do not build MSI, multiple installer formats, or updater artifacts locally unless explicitly requested.
- GitHub tag releases build only the signed NSIS executable and reuse the Rust cache. Use the full multi-bundle build only when explicitly requested.

## Architecture

**Two independent frontend entry points**, each with its own Vite/React root, sharing `App.css`:
- `index.html` / `src/main.tsx` → `App.tsx`—tray popup; renders a `ProviderCard` for each enabled provider.
- `settings.html` / `src/settings-main.tsx` → `Settings.tsx`—refresh interval, provider toggles, proxy mode, launch-at-startup, and the updater.

The frontend talks to the backend only through `src/api.ts`: `get_usage`,
`refresh_usage`, `get_settings`, `save_settings`, `open_settings_window`,
`list_providers`, `get_proxy_status`, `refresh_provider` (one card),
`fit_panel` (the panel reports its content height and the window follows).
Live data arrives via the `usage-updated` event, not frontend polling; saved
settings are pushed to every window as `settings-updated`, and `panel-shown`
fires each time the panel opens (it refreshes if its data is older than two
intervals). UI strings live in `src/i18n.ts` (English text is the key, zh
table alongside); `src/hooks.ts` holds the shared settings/i18n/clock hooks.

### Backend layout (`src-tauri/src/`)

Each file owns exactly one concern; `lib.rs` is assembly only.

| File | Responsibility |
|---|---|
| `lib.rs` | Builds the Tauri app and wires the pieces together |
| `models.rs` | Shared types (`UsageSnapshot`, `UsageMetric`, `AppSettings`, `ProxyMode`) |
| `config.rs` | Loading/saving `settings.json` and `alert-marks.json` in the app-config dir |
| `state.rs` | `AppState` plus `refresh()`/`refresh_provider()`—the only paths a refresh takes; both end in `publish()` (cache, alerts, tray tooltip, `usage-updated`) |
| `quota.rs` | Tauri-free, unit-tested logic: `due_alerts` (once per window cycle) and `tooltip_summary` |
| `locale.rs` | zh/en choice for backend-shown strings (tray menu, notifications, window titles) |
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

Build percentage windows with `UsageMetric::percent(label, pct, reset_at)` plus
`.spanning(window_secs)` or `.starting(start)`, so the UI can compute pace and a
run-out estimate. Label windows by their real length (`models::window_label`,
e.g. "5h"/"7d"/"30d"), not by the vendor's slot name—Codex's "primary" window
is the weekly one on some plans. Attach `.with_plan()`/`.with_account()` when
the source exposes them.

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
uses the address from Settings. HTTP and SOCKS (`socks5://`, `socks5h://`, …)
proxies are supported; a registry `socks=` entry maps to `socks5h://`. An
address `reqwest` cannot use resolves to `ProxySource::Invalid` rather than
silently going direct. The resolved source is surfaced in the Settings window
via `get_proxy_status`.

### App lifecycle

`tauri-plugin-single-instance` is registered first in `lib.rs`: a second launch
shows the existing panel instead of adding a second tray icon and polling loop.
The tray panel hides on blur; `window::toggle` ignores a toggle that arrives
right after a blur-hide, because that is the same tray click that caused it.

The popup does not trigger a refresh on mount—the scheduler's first cycle is
already running at launch—so the frontend reads the cache and waits for the
`usage-updated` push.

Full refreshes are serialised through `refresh_gate`; one requested within 5 s
of the last returns the cache instead of hitting every API again. After each
refresh, `publish()` sends a desktop notification for any window that crossed
`alert_threshold` (once per window cycle; marks persist in `alert-marks.json`)
and rewrites the tray tooltip summary. `save_settings` wakes the scheduler only
when the interval, providers or proxy changed—display-only settings (language,
used/left, alerts) do not trigger a fetch.

`get_settings` reports `autostart` from the OS (`autolaunch().is_enabled()`),
not the stored flag, since the user can change it outside the app.

### Shared types

`UsageSnapshot`, `UsageMetric`, `AppSettings`, `ProxyMode` are manually kept in
sync between `models.rs` and `types.ts`; update both when changing structures.
`AppSettings` carries a struct-level `#[serde(default)]`, so a config missing
any field keeps its other values instead of being replaced by defaults.

`UsageSnapshot::or_previous` carries the last good metrics (flagged `stale`)
through an `Error` only; `NotConnected` and `NoQuotaApi` are shown as they are.

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

**Factory** reads `GET /api/billing/limits` (what `droid /status` shows): rolling
5h / 7d / 30d windows for the standard pool, plus a Droid Core pool shown only
when used, and extra-usage balance. The older
`/api/organization/subscription/usage` still answers for those accounts but with
a token allowance that no longer governs them (it read 271% used for an account
at 21% of its 5-hour window); it is only a fallback for accounts with
`usesTokenRateLimitsBilling: false`.

**Grok**'s billing response is gRPC-web framed protobuf, scanned shallowly by
field path. `parse_billing_response` takes `now` as a parameter rather than
reading the clock, so captured responses stay replayable in tests instead of
rotting once their reset time slips into the past.

### Known Cross-Platform Gaps

The project is developed and distributed for Windows; adapter path resolution is
otherwise platform-agnostic. One known issue would break on macOS:
- `adapters/claude.rs` only reads `~/.claude/.credentials.json`; on macOS Claude Code stores OAuth credentials in the Keychain, so that file often does not exist even when logged in.
