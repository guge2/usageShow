import { useEffect, useState } from "react";
import {
  checkForAppUpdate,
  getAppVersion,
  getProxyStatus,
  getSettings,
  installAppUpdate,
  listProviders,
  relaunchApp,
  saveSettings,
} from "./api";
import type { AppUpdateProgress, AvailableAppUpdate } from "./api";
import { PROXY_MODE_OPTIONS, REFRESH_INTERVAL_OPTIONS } from "./types";
import type { AppSettings, ProviderInfo, ProxyMode, ProxyStatus } from "./types";
import "./App.css";
import "./Settings.css";

function Settings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [currentVersion, setCurrentVersion] = useState("");
  const [availableUpdate, setAvailableUpdate] =
    useState<AvailableAppUpdate | null>(null);
  const [updateStatus, setUpdateStatus] = useState<
    | "idle"
    | "checking"
    | "latest"
    | "available"
    | "downloading"
    | "restarting"
    | "error"
  >("idle");
  const [updateMessage, setUpdateMessage] = useState(
    "Check GitHub Releases for a signed update.",
  );
  const [updateProgress, setUpdateProgress] =
    useState<AppUpdateProgress | null>(null);

  useEffect(() => {
    void getSettings().then(setSettings);
    void listProviders().then(setProviders);
    void refreshProxyStatus();
    getAppVersion()
      .then(setCurrentVersion)
      .catch(() => setCurrentVersion("Unknown"));
  }, []);

  function refreshProxyStatus() {
    return getProxyStatus()
      .then(setProxyStatus)
      .catch(() => setProxyStatus(null));
  }

  async function updateSettings(next: AppSettings) {
    setSettings(next);
    await saveSettings(next);
    // Re-read what the backend resolved, so the proxy line reflects the change
    // that was actually saved rather than what the UI assumed.
    await refreshProxyStatus();
  }

  function toggleProvider(id: string) {
    if (!settings) return;
    const enabled = settings.enabled_providers.includes(id)
      ? settings.enabled_providers.filter((p) => p !== id)
      : [...settings.enabled_providers, id];
    void updateSettings({ ...settings, enabled_providers: enabled });
  }

  async function checkForUpdates() {
    setUpdateStatus("checking");
    setUpdateMessage("Checking for updates...");
    setUpdateProgress(null);

    try {
      if (availableUpdate) {
        await availableUpdate.close();
        setAvailableUpdate(null);
      }

      const nextUpdate = await checkForAppUpdate();
      if (!nextUpdate) {
        setUpdateStatus("latest");
        setUpdateMessage("You're using the latest version.");
        return;
      }

      setAvailableUpdate(nextUpdate);
      setUpdateStatus("available");
      setUpdateMessage(`Version ${nextUpdate.version} is ready to install.`);
    } catch (error) {
      setUpdateStatus("error");
      setUpdateMessage(
        `Couldn't check for updates: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }

  async function installUpdate() {
    if (!availableUpdate) return;

    setUpdateStatus("downloading");
    setUpdateMessage(`Downloading version ${availableUpdate.version}...`);
    setUpdateProgress({ downloaded: 0 });

    try {
      await installAppUpdate(availableUpdate, setUpdateProgress);
      setUpdateStatus("restarting");
      setUpdateMessage("Update installed. Restarting...");
      await relaunchApp();
    } catch (error) {
      setUpdateStatus("error");
      setUpdateMessage(
        `Couldn't install the update: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }

  const progressPercent =
    updateProgress?.total && updateProgress.total > 0
      ? Math.min(
          100,
          Math.round((updateProgress.downloaded / updateProgress.total) * 100),
        )
      : null;

  if (!settings) {
    return (
      <div className="settings-shell">
        <p className="empty-state">Loading...</p>
      </div>
    );
  }

  return (
    <div className="settings-shell">
      <section className="settings-section">
        <h3>Refresh interval</h3>
        <select
          className="settings-select"
          value={settings.refresh_interval_secs}
          onChange={(e) =>
            void updateSettings({
              ...settings,
              refresh_interval_secs: Number(e.target.value),
            })
          }
        >
          {REFRESH_INTERVAL_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </section>

      <section className="settings-section">
        <h3>Visible tools</h3>
        <div className="checkbox-list">
          {providers.map((p) => (
            <label key={p.id} className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.enabled_providers.includes(p.id)}
                onChange={() => toggleProvider(p.id)}
              />
              <span>{p.display_name}</span>
            </label>
          ))}
        </div>
      </section>

      <section className="settings-section">
        <h3>Network</h3>
        <select
          className="settings-select"
          value={settings.proxy_mode}
          onChange={(e) =>
            void updateSettings({
              ...settings,
              proxy_mode: e.target.value as ProxyMode,
            })
          }
        >
          {PROXY_MODE_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        <p className="settings-hint">
          {
            PROXY_MODE_OPTIONS.find((o) => o.value === settings.proxy_mode)
              ?.hint
          }
        </p>

        {settings.proxy_mode === "manual" && (
          <input
            className="settings-input"
            type="text"
            spellCheck={false}
            placeholder="127.0.0.1:7890"
            defaultValue={settings.proxy_url}
            onBlur={(e) =>
              void updateSettings({ ...settings, proxy_url: e.target.value.trim() })
            }
          />
        )}

        {proxyStatus && (
          <p
            className={`proxy-status${proxyStatus.active ? " proxy-status-active" : ""}`}
            aria-live="polite"
          >
            {proxyStatus.description}
          </p>
        )}
      </section>

      <section className="settings-section">
        <h3>Startup</h3>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) =>
              void updateSettings({ ...settings, autostart: e.target.checked })
            }
          />
          <span>Launch automatically at Windows startup</span>
        </label>
      </section>

      <section className="settings-section">
        <div className="settings-section-heading">
          <h3>Updates</h3>
          <span className="version-label">
            {currentVersion ? `v${currentVersion}` : "Loading..."}
          </span>
        </div>

        <p
          className={`update-message update-message-${updateStatus}`}
          aria-live="polite"
        >
          {updateMessage}
        </p>

        {availableUpdate?.body && updateStatus === "available" && (
          <p className="update-notes">{availableUpdate.body}</p>
        )}

        {updateStatus === "downloading" && (
          <div className="update-progress">
            <div className="update-progress-track">
              <div
                className={`update-progress-fill${progressPercent === null ? " update-progress-indeterminate" : ""}`}
                style={
                  progressPercent === null
                    ? undefined
                    : { width: `${progressPercent}%` }
                }
              />
            </div>
            {progressPercent !== null && <span>{progressPercent}%</span>}
          </div>
        )}

        <button
          className="update-button"
          type="button"
          disabled={
            updateStatus === "checking" ||
            updateStatus === "downloading" ||
            updateStatus === "restarting"
          }
          onClick={() =>
            void (updateStatus === "available"
              ? installUpdate()
              : checkForUpdates())
          }
        >
          {updateStatus === "checking"
            ? "Checking..."
            : updateStatus === "downloading"
              ? "Downloading..."
              : updateStatus === "restarting"
                ? "Restarting..."
                : updateStatus === "available"
                  ? `Install v${availableUpdate?.version}`
                  : "Check for updates"}
        </button>
      </section>
    </div>
  );
}

export default Settings;
