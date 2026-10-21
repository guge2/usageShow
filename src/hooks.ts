import { useEffect, useMemo, useState } from "react";
import { getSettings, onSettingsUpdated } from "./api";
import { makeT, resolveLang, type Lang, type T } from "./i18n";
import type { AppSettings } from "./types";

/** Settings kept in sync across windows via the `settings-updated` event. */
export function useSettings(): [AppSettings | null, (s: AppSettings) => void] {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  useEffect(() => {
    getSettings().then(setSettings);
    const unlisten = onSettingsUpdated(setSettings);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);
  return [settings, setSettings];
}

export function useI18n(settings: AppSettings | null): { lang: Lang; t: T } {
  const lang = resolveLang(settings?.language);
  useEffect(() => {
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  }, [lang]);
  return useMemo(() => ({ lang, t: makeT(lang) }), [lang]);
}

/** Re-render periodically so relative times ("5m ago", pace) stay current. */
export function useNow(intervalMs = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
