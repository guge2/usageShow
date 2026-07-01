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
];

export const REFRESH_INTERVAL_OPTIONS: { value: number; label: string }[] = [
  { value: 60, label: "1 分钟" },
  { value: 180, label: "3 分钟" },
  { value: 300, label: "5 分钟" },
  { value: 600, label: "10 分钟" },
  { value: 900, label: "15 分钟" },
  { value: 1800, label: "30 分钟" },
];
