export type UsageStatus = "ok" | "not_connected" | "error";

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
}

export interface AppSettings {
  refresh_interval_secs: number;
  enabled_providers: string[];
  autostart: boolean;
}

export const ALL_PROVIDERS: { id: string; label: string }[] = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "cursor", label: "Cursor" },
  { id: "amp", label: "Amp" },
  { id: "factory", label: "Factory Droid" },
  { id: "agy", label: "AGY" },
];

export const REFRESH_INTERVAL_OPTIONS: { value: number; label: string }[] = [
  { value: 60, label: "1 minute" },
  { value: 180, label: "3 minutes" },
  { value: 300, label: "5 minutes" },
  { value: 600, label: "10 minutes" },
  { value: 900, label: "15 minutes" },
  { value: 1800, label: "30 minutes" },
];
