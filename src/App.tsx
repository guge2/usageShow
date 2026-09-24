import { useEffect, useRef, useState, useCallback } from "react";
import {
  getSettings,
  getUsage,
  refreshUsage,
  onUsageUpdated,
  openSettingsWindow,
} from "./api";
import type { UsageSnapshot } from "./types";
import { ProviderCard } from "./components/ProviderCard";
import { formatUpdatedAt } from "./utils";
import "./App.css";

function RefreshIcon({ spinning }: { spinning: boolean }) {
  return (
    <svg
      className={spinning ? "spin" : ""}
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M21 12a9 9 0 1 1-2.64-6.36" />
      <path d="M21 3v6h-6" />
    </svg>
  );
}

function GearIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function App() {
  const [snapshots, setSnapshots] = useState<UsageSnapshot[]>([]);
  // Whether a refresh result has arrived, so an empty list can be told apart
  // from "still loading" (it is also what every provider being disabled looks like).
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const receivedPush = useRef(false);

  const applyFresh = useCallback((fresh: UsageSnapshot[]) => {
    receivedPush.current = true;
    setSnapshots(fresh);
    setLoaded(true);
  }, []);

  useEffect(() => {
    // This window is created at app launch, while the backend scheduler is
    // already running its first refresh. Triggering another refresh here would
    // hit every provider twice at startup, so just listen for its result and
    // show whatever is cached meanwhile.
    const unlisten = onUsageUpdated(applyFresh);
    unlisten
      .then(() => getUsage())
      .then((cached) => {
        // A pushed update may have landed first; it is newer than the cache read.
        if (receivedPush.current) return;
        if (cached.length > 0) {
          setSnapshots(cached);
          setLoaded(true);
          return;
        }
        // An empty cache means either no refresh has finished yet, or nothing
        // is enabled - in which case no push with data will ever arrive.
        return getSettings().then((settings) => {
          if (!receivedPush.current && settings.enabled_providers.length === 0) {
            setLoaded(true);
          }
        });
      });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [applyFresh]);

  const handleRefresh = useCallback(async () => {
    setLoading(true);
    try {
      applyFresh(await refreshUsage());
    } finally {
      setLoading(false);
    }
  }, [applyFresh]);

  const lastUpdated =
    snapshots.length > 0 ? Math.max(...snapshots.map((s) => s.updated_at)) : null;

  return (
    <div className="app-shell">
      <header className="app-header" data-tauri-drag-region>
        <span className="app-title">AI Usage Monitor</span>
        <div className="header-actions">
          <button
            className="icon-button"
            onClick={handleRefresh}
            disabled={loading}
            aria-label="Refresh"
            title="Refresh"
          >
            <RefreshIcon spinning={loading} />
          </button>
          <button
            className="icon-button"
            onClick={() => openSettingsWindow()}
            aria-label="Settings"
            title="Settings"
          >
            <GearIcon />
          </button>
        </div>
      </header>

      <main className="app-body">
        {snapshots.length === 0 ? (
          <p className="empty-state">
            {loaded ? "No tools enabled. Choose some in Settings." : "Loading..."}
          </p>
        ) : (
          snapshots.map((s) => <ProviderCard key={s.provider} snapshot={s} />)
        )}
      </main>

      <footer className="app-footer">
        <span>{lastUpdated ? `Last updated ${formatUpdatedAt(lastUpdated)}` : ""}</span>
      </footer>
    </div>
  );
}

export default App;
