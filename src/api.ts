import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppSettings, UsageSnapshot } from "./types";

export function getUsage(): Promise<UsageSnapshot[]> {
  return invoke("get_usage");
}

export function refreshUsage(): Promise<UsageSnapshot[]> {
  return invoke("refresh_usage");
}

export function onUsageUpdated(cb: (data: UsageSnapshot[]) => void) {
  return listen<UsageSnapshot[]>("usage-updated", (event) => cb(event.payload));
}

export function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export function saveSettings(settings: AppSettings): Promise<void> {
  return invoke("save_settings", { settings });
}

export function openSettingsWindow(): Promise<void> {
  return invoke("open_settings_window");
}
