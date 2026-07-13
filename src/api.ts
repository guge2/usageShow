import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import type { AppSettings, UsageSnapshot } from "./types";

export interface AppUpdateProgress {
  downloaded: number;
  total?: number;
}

export type AvailableAppUpdate = Update;

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

export function getAppVersion(): Promise<string> {
  return getVersion();
}

export function checkForAppUpdate(): Promise<AvailableAppUpdate | null> {
  return check({ timeout: 30_000 });
}

export function installAppUpdate(
  update: AvailableAppUpdate,
  onProgress: (progress: AppUpdateProgress) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | undefined;

  return update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength;
      onProgress({ downloaded, total });
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      onProgress({ downloaded, total });
    } else {
      onProgress({ downloaded: total ?? downloaded, total });
    }
  });
}

export function relaunchApp(): Promise<void> {
  return relaunch();
}
