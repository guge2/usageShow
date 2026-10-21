import type { Lang, T } from "./i18n";
import type { UsageMetric } from "./types";

const DAY_MS = 86_400_000;

function locale(lang: Lang): string {
  return lang === "zh" ? "zh-CN" : "en-US";
}

function clock(d: Date, lang: Lang): string {
  return d.toLocaleTimeString(locale(lang), { hour: "2-digit", minute: "2-digit", hour12: false });
}

/**
 * When a window resets, on the clock: "14:30" today, "tomorrow 09:00",
 * "Thu 14:30" later this week, else "Oct 12 08:05".
 */
export function resetClock(resetAt: number, lang: Lang, t: T, now = new Date()): string {
  const at = new Date(resetAt * 1000);
  const time = clock(at, lang);
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOfDay(at) - startOfDay(now)) / DAY_MS);
  if (days <= 0) return time;
  if (days === 1) return t("tomorrow {time}", { time });
  // Monday-based week: days left until Sunday
  const daysLeftInWeek = 6 - ((now.getDay() + 6) % 7);
  if (days <= daysLeftInWeek) {
    return `${at.toLocaleDateString(locale(lang), { weekday: "short" })} ${time}`;
  }
  return `${at.toLocaleDateString(locale(lang), { month: "short", day: "numeric" })} ${time}`;
}

/** "in 45m" / "in 3h" / "in 5d". */
export function untilText(ts: number, t: T, nowMs = Date.now()): string {
  const diff = ts * 1000 - nowMs;
  if (diff <= 0) return t("Reset time passed");
  const mins = Math.max(1, Math.round(diff / 60_000));
  if (mins < 60) return t("in {n}m", { n: mins });
  const hours = Math.round(mins / 60);
  if (hours < 48) return t("in {n}h", { n: hours });
  return t("in {n}d", { n: Math.round(hours / 24) });
}

/** "Just now" / "5m ago" / "3h ago" / "2d ago". */
export function agoText(ts: number, t: T, nowMs = Date.now()): string {
  const mins = Math.floor((nowMs - ts * 1000) / 60_000);
  if (mins < 1) return t("Just now");
  if (mins < 60) return t("{n}m ago", { n: mins });
  const hours = Math.floor(mins / 60);
  if (hours < 48) return t("{n}h ago", { n: hours });
  return t("{n}d ago", { n: Math.floor(hours / 24) });
}

/** Clock time for "today", otherwise date + time. */
export function timeText(ts: number, lang: Lang, nowMs = Date.now()): string {
  const d = new Date(ts * 1000);
  if (d.toDateString() === new Date(nowMs).toDateString()) return clock(d, lang);
  return `${d.toLocaleDateString(locale(lang), { month: "short", day: "numeric" })} ${clock(d, lang)}`;
}

export function formatUpdatedAt(ts: number, lang: Lang): string {
  return clock(new Date(ts * 1000), lang);
}

export function formatAmount(value: number, unit: string): string {
  if (unit === "usd") return `$${value.toFixed(2)}`;
  if (unit === "percent") return `${value.toFixed(0)}%`;
  if (unit === "tokens" || unit === "credits") return formatCompactNumber(value);
  return `${Math.round(value)}`;
}

function formatCompactNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 10_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toLocaleString("en-US", { maximumFractionDigits: value >= 100 ? 0 : 2 });
}

export function usedPercent(m: UsageMetric): number | null {
  if (m.percent === null || !Number.isFinite(m.percent)) return null;
  return Math.max(0, Math.min(100, m.percent));
}

export interface Pace {
  /** Points of the window used beyond (+) or below (−) an even pace. */
  delta: number;
  over: boolean;
}

/**
 * How far a window is from an even burn through it: used% minus the share
 * of the window already elapsed. `null` when the window's start or reset is
 * unknown, or `now` is outside it.
 */
export function windowPace(m: UsageMetric, nowMs = Date.now()): Pace | null {
  const used = usedPercent(m);
  if (used === null || m.starts_at === null || m.reset_at === null) return null;
  const start = m.starts_at * 1000;
  const end = m.reset_at * 1000;
  if (!(end > start) || nowMs < start || nowMs >= end) return null;
  const even = (100 * (nowMs - start)) / (end - start);
  const delta = Math.round(used - even);
  return { delta, over: delta > 0 };
}

/** "+12%" / "−8%" (a true minus) / "±0%". */
export function paceNumber(n: number): string {
  return `${n > 0 ? "+" : n < 0 ? "−" : "±"}${Math.abs(n)}%`;
}

export type RunOut = { lasts: true } | { lasts: false; at: number };

/**
 * Where the window's average burn rate so far takes it: the time it would
 * hit 100%, or that it lasts past the reset. `null` early in a window (too
 * little to go on) or when nothing has been used.
 */
export function runOut(m: UsageMetric, nowMs = Date.now()): RunOut | null {
  const used = usedPercent(m);
  if (used === null || m.starts_at === null || m.reset_at === null) return null;
  const start = m.starts_at * 1000;
  const end = m.reset_at * 1000;
  const elapsed = nowMs - start;
  if (!(end > start) || nowMs >= end || used <= 0 || used >= 100) return null;
  if (elapsed < Math.max(15 * 60_000, (end - start) * 0.05)) return null;
  const at = nowMs + ((100 - used) / used) * elapsed;
  return at >= end ? { lasts: true } : { lasts: false, at: Math.floor(at / 1000) };
}

export function severity(percentUsed: number): "ok" | "warn" | "danger" {
  if (percentUsed >= 90) return "danger";
  if (percentUsed >= 70) return "warn";
  return "ok";
}
