/**
 * Mirrors `src-tauri/src/models.rs`. Keep the two in sync when changing shapes.
 *
 * The provider list deliberately does NOT live here — it is fetched from the
 * backend via `listProviders()`, so registering a provider in Rust is the only
 * step required.
 */

export type UsageStatus = "ok" | "not_connected" | "error" | "no_quota_api";

export interface UsageMetric {
  label: string;
  used: number;
  limit: number | null;
  percent: number | null;
  /** "percent" | "usd" | "requests" | "tokens" | "credits" */
  unit: string;
  /** Unix seconds when this window resets. */
  reset_at: number | null;
  /** Unix seconds when the current window started (enables pace). */
  starts_at: number | null;
}

export interface UsageSnapshot {
  provider: string;
  display_name: string;
  status: UsageStatus;
  message: string | null;
  metrics: UsageMetric[];
  /** Unix seconds when `metrics` were read. */
  updated_at: number;
  /** Metrics carried over from an earlier fetch because this one failed. */
  stale: boolean;
  plan: string | null;
  account: string | null;
}

export interface ProviderInfo {
  id: string;
  display_name: string;
}

export type ProxyMode = "auto" | "off" | "manual";

export type LanguageSetting = "auto" | "en" | "zh";

export interface AppSettings {
  refresh_interval_secs: number;
  enabled_providers: string[];
  autostart: boolean;
  proxy_mode: ProxyMode;
  proxy_url: string;
  show_remaining: boolean;
  /** 0 disables alerts. */
  alert_threshold: number;
  language: LanguageSetting;
}

/** What the backend resolved the proxy setting to, for display only. */
export interface ProxyStatus {
  description: string;
  active: boolean;
}

export const REFRESH_INTERVAL_OPTIONS: number[] = [60, 180, 300, 600, 900, 1800];

export const ALERT_THRESHOLD_OPTIONS: number[] = [0, 70, 80, 90, 95];

/** Labels and hints are English i18n keys. */
export const PROXY_MODE_OPTIONS: { value: ProxyMode; label: string; hint: string }[] = [
  {
    value: "auto",
    label: "Automatic",
    hint: "Use HTTPS_PROXY/HTTP_PROXY, then the Windows system proxy.",
  },
  { value: "off", label: "Direct", hint: "Never use a proxy." },
  { value: "manual", label: "Manual", hint: "Always use the address below." },
];
