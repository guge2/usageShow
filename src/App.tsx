import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  fitPanel,
  getUsage,
  onPanelShown,
  onUsageUpdated,
  openSettingsWindow,
  refreshProvider,
  refreshUsage,
  saveSettings,
} from "./api";
import type { UsageSnapshot } from "./types";
import { ProviderCard } from "./components/ProviderCard";
import { GearIcon, RefreshIcon } from "./components/Icons";
import { useI18n, useNow, useSettings } from "./hooks";
import { formatUpdatedAt } from "./utils";
import "./App.css";

function App() {
  const [snapshots, setSnapshots] = useState<UsageSnapshot[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [settings, setSettings] = useSettings();
  const { lang, t } = useI18n(settings);
  const now = useNow();

  const headerRef = useRef<HTMLElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  const footerRef = useRef<HTMLElement>(null);

  // The backend refreshes on its own schedule and pushes `usage-updated`;
  // on mount just take what it has (including the last run's readings).
  useEffect(() => {
    getUsage().then((cached) => setSnapshots((cur) => cur ?? cached));
    const unlisten = onUsageUpdated(setSnapshots);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleRefresh = useCallback(async () => {
    setLoading(true);
    try {
      setSnapshots(await refreshUsage());
    } finally {
      setLoading(false);
    }
  }, []);

  const handleRefreshOne = useCallback(async (provider: string) => {
    setSnapshots(await refreshProvider(provider));
  }, []);

  // Opening the panel on data older than two refresh intervals (e.g. after
  // the computer slept) triggers a refresh.
  const latestRef = useRef({ snapshots, settings, handleRefresh });
  latestRef.current = { snapshots, settings, handleRefresh };
  useEffect(() => {
    const unlisten = onPanelShown(() => {
      const { snapshots: snaps, settings: s, handleRefresh: refresh } = latestRef.current;
      if (!snaps?.length || !s) return;
      const newest = Math.max(...snaps.map((x) => x.updated_at));
      if (Date.now() / 1000 - newest > s.refresh_interval_secs * 2) void refresh();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const toggleMode = useCallback(() => {
    if (!settings) return;
    const next = { ...settings, show_remaining: !settings.show_remaining };
    setSettings(next);
    void saveSettings(next);
  }, [settings, setSettings]);

  // Size the window to its content, so a couple of tools don't sit in a
  // mostly empty panel and many tools get more room (up to a cap).
  useLayoutEffect(() => {
    const inner = innerRef.current;
    if (!inner) return;
    const measure = () => {
      const header = headerRef.current?.offsetHeight ?? 0;
      const footer = footerRef.current?.offsetHeight ?? 0;
      const bodyPadding = 16;
      const border = 2;
      void fitPanel(header + footer + inner.scrollHeight + bodyPadding + border);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(inner);
    measure();
    return () => observer.disconnect();
  }, []);

  const showRemaining = settings?.show_remaining ?? false;
  const list = snapshots ?? [];
  const connected = list.filter((s) => s.status !== "not_connected");
  const notConnected = list.filter((s) => s.status === "not_connected");
  const lastUpdated = list.length > 0 ? Math.max(...list.map((s) => s.updated_at)) : null;

  return (
    <div className="app-shell">
      <header className="app-header" data-tauri-drag-region ref={headerRef}>
        <span className="app-title" data-tauri-drag-region>
          {t("AI Usage Monitor")}
        </span>
        <div className="header-actions">
          {connected.length > 0 && (
            <button
              type="button"
              className="mode-pill"
              onClick={toggleMode}
              title={t(showRemaining ? "Show how much is used" : "Show how much is left")}
            >
              {t(showRemaining ? "Left" : "Used")}
            </button>
          )}
          <button
            type="button"
            className="icon-button"
            onClick={handleRefresh}
            disabled={loading}
            aria-label={t("Refresh")}
            title={t("Refresh")}
          >
            <RefreshIcon spinning={loading} />
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={() => openSettingsWindow()}
            aria-label={t("Settings")}
            title={t("Settings")}
          >
            <GearIcon />
          </button>
        </div>
      </header>

      <main className="app-body">
        <div className="app-body-inner" ref={innerRef}>
          {snapshots === null || (list.length === 0 && loading) ? (
            <p className="empty-state">{t("Loading...")}</p>
          ) : list.length === 0 ? (
            <p className="empty-state">
              {settings?.enabled_providers.length === 0
                ? t("No tools enabled. Choose some in Settings.")
                : t("Loading...")}
            </p>
          ) : (
            <>
              {connected.map((s) => (
                <ProviderCard
                  key={s.provider}
                  snapshot={s}
                  showRemaining={showRemaining}
                  onToggleMode={toggleMode}
                  onRefresh={handleRefreshOne}
                  t={t}
                  lang={lang}
                  now={now}
                />
              ))}
              {notConnected.length > 0 && (
                <p
                  className="not-connected"
                  title={notConnected.map((s) => `${s.display_name}: ${t(s.message ?? "")}`).join("\n")}
                >
                  {t("Not connected: {names}", {
                    names: notConnected.map((s) => s.display_name).join(lang === "zh" ? "、" : ", "),
                  })}
                </p>
              )}
            </>
          )}
        </div>
      </main>

      <footer className="app-footer" ref={footerRef}>
        <span>{lastUpdated ? t("Last updated {time}", { time: formatUpdatedAt(lastUpdated, lang) }) : ""}</span>
      </footer>
    </div>
  );
}

export default App;
