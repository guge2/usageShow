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
  unit: string;
  reset_at: number | null;
}

export interface UsageSnapshot {
  provider: string;
  display_name: string;
  status: UsageStatus;
  message: string | null;
  metrics: UsageMetric[];
  updated_at: number;
  /** Metrics carried over from an earlier fetch because this one failed. */
  stale: boolean;
}

export interface ProviderInfo {
  id: string;
  display_name: string;
}

export type ProxyMode = "auto" | "off" | "manual";

export interface AppSettings {
  refresh_interval_secs: number;
  enabled_providers: string[];
  autostart: boolean;
  proxy_mode: ProxyMode;
  proxy_url: string;
}

/** What the backend resolved the proxy setting to, for display only. */
export interface ProxyStatus {
  description: string;
  active: boolean;
}

export const REFRESH_INTERVAL_OPTIONS: { value: number; label: string }[] = [
  { value: 60, label: "1 minute" },
  { value: 180, label: "3 minutes" },
  { value: 300, label: "5 minutes" },
  { value: 600, label: "10 minutes" },
  { value: 900, label: "15 minutes" },
  { value: 1800, label: "30 minutes" },
];

export const PROXY_MODE_OPTIONS: { value: ProxyMode; label: string; hint: string }[] = [
  {
    value: "auto",
    label: "Automatic",
    hint: "Use HTTPS_PROXY/HTTP_PROXY, then the Windows system proxy.",
  },
  { value: "off", label: "Direct", hint: "Never use a proxy." },
  { value: "manual", label: "Manual", hint: "Always use the address below." },
];
