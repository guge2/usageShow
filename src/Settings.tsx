import { useEffect, useState } from "react";
import {
  checkForAppUpdate,
  getAppVersion,
  getProxyStatus,
  installAppUpdate,
  listProviders,
  relaunchApp,
  saveSettings,
} from "./api";
import type { AppUpdateProgress, AvailableAppUpdate } from "./api";
import { useI18n, useSettings } from "./hooks";
import { ALERT_THRESHOLD_OPTIONS, PROXY_MODE_OPTIONS, REFRESH_INTERVAL_OPTIONS } from "./types";
import type { AppSettings, LanguageSetting, ProviderInfo, ProxyMode, ProxyStatus } from "./types";
import "./App.css";
import "./Settings.css";

type UpdateStatus =
  | "idle"
  | "checking"
  | "latest"
  | "available"
  | "downloading"
  | "restarting"
  | "error";

/** An i18n key plus its variables, so messages re-translate on language change. */
interface Message {
  key: string;
  vars?: Record<string, string | number>;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function Settings() {
  const [settings, setSettings] = useSettings();
  const { t } = useI18n(settings);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [currentVersion, setCurrentVersion] = useState("");
  const [availableUpdate, setAvailableUpdate] = useState<AvailableAppUpdate | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>("idle");
  const [updateMessage, setUpdateMessage] = useState<Message>({
    key: "Check GitHub Releases for a signed update.",
  });
  const [updateProgress, setUpdateProgress] = useState<AppUpdateProgress | null>(null);

  useEffect(() => {
    void listProviders().then(setProviders);
    void refreshProxyStatus();
    getAppVersion()
      .then(setCurrentVersion)
      .catch(() => setCurrentVersion(""));
  }, []);

  function refreshProxyStatus() {
    return getProxyStatus()
      .then(setProxyStatus)
      .catch(() => setProxyStatus(null));
  }

  async function updateSettings(next: AppSettings) {
    const previous = settings;
    setSettings(next);
    try {
      await saveSettings(next);
      setSaveError(null);
    } catch (error) {
      // Nothing was persisted (e.g. the OS refused launch-at-startup), so put
      // the controls back to what is actually in effect.
      if (previous) setSettings(previous);
      setSaveError(errorText(error));
      return;
    }
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
    setUpdateMessage({ key: "Checking for updates..." });
    setUpdateProgress(null);

    try {
      if (availableUpdate) {
        await availableUpdate.close();
        setAvailableUpdate(null);
      }

      const nextUpdate = await checkForAppUpdate();
      if (!nextUpdate) {
        setUpdateStatus("latest");
        setUpdateMessage({ key: "You're using the latest version." });
        return;
      }

      setAvailableUpdate(nextUpdate);
      setUpdateStatus("available");
      setUpdateMessage({ key: "Version {v} is ready to install.", vars: { v: nextUpdate.version } });
    } catch (error) {
      setUpdateStatus("error");
      setUpdateMessage({ key: "Couldn't check for updates: {e}", vars: { e: errorText(error) } });
    }
  }

  async function installUpdate() {
    if (!availableUpdate) return;

    setUpdateStatus("downloading");
    setUpdateMessage({ key: "Downloading version {v}...", vars: { v: availableUpdate.version } });
    setUpdateProgress({ downloaded: 0 });

    try {
      await installAppUpdate(availableUpdate, setUpdateProgress);
      setUpdateStatus("restarting");
      setUpdateMessage({ key: "Update installed. Restarting..." });
      await relaunchApp();
    } catch (error) {
      setUpdateStatus("error");
      setUpdateMessage({ key: "Couldn't install the update: {e}", vars: { e: errorText(error) } });
    }
  }

  const progressPercent =
    updateProgress?.total && updateProgress.total > 0
      ? Math.min(100, Math.round((updateProgress.downloaded / updateProgress.total) * 100))
      : null;

  if (!settings) {
    return (
      <div className="settings-shell">
        <p className="empty-state">{t("Loading...")}</p>
      </div>
    );
  }

  return (
    <div className="settings-shell">
      {saveError && (
        <p className="settings-error" role="alert">
          {t("Couldn't save settings: {e}", { e: saveError })}
        </p>
      )}

      <section className="settings-section">
        <h3>{t("Display")}</h3>
        <label className="field-row">
          <span>{t("Language")}</span>
          <select
            className="settings-select"
            value={settings.language}
            onChange={(e) =>
              void updateSettings({ ...settings, language: e.target.value as LanguageSetting })
            }
          >
            <option value="auto">{t("Follow system")}</option>
            <option value="zh">中文</option>
            <option value="en">English</option>
          </select>
        </label>
        <label className="field-row">
          <span>{t("Rings show")}</span>
          <select
            className="settings-select"
            value={settings.show_remaining ? "left" : "used"}
            onChange={(e) =>
              void updateSettings({ ...settings, show_remaining: e.target.value === "left" })
            }
          >
            <option value="used">{t("Used quota")}</option>
            <option value="left">{t("Remaining quota")}</option>
          </select>
        </label>
      </section>

      <section className="settings-section">
        <h3>{t("Usage alerts")}</h3>
        <label className="field-row">
          <span>{t("Notify when a window reaches")}</span>
          <select
            className="settings-select"
            value={settings.alert_threshold}
            onChange={(e) =>
              void updateSettings({ ...settings, alert_threshold: Number(e.target.value) })
            }
          >
            {ALERT_THRESHOLD_OPTIONS.map((v) => (
              <option key={v} value={v}>
                {v === 0 ? t("Off") : `${v}%`}
              </option>
            ))}
          </select>
        </label>
        <p className="settings-hint">{t("Each window alerts once per cycle.")}</p>
      </section>

      <section className="settings-section">
        <h3>{t("Refresh interval")}</h3>
        <select
          className="settings-select"
          value={settings.refresh_interval_secs}
          onChange={(e) =>
            void updateSettings({ ...settings, refresh_interval_secs: Number(e.target.value) })
          }
        >
          {REFRESH_INTERVAL_OPTIONS.map((secs) => {
            const n = secs / 60;
            return (
              <option key={secs} value={secs}>
                {t(n === 1 ? "{n} minute" : "{n} minutes", { n })}
              </option>
            );
          })}
        </select>
      </section>

      <section className="settings-section">
        <h3>{t("Visible tools")}</h3>
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
        <h3>{t("Network")}</h3>
        <select
          className="settings-select"
          value={settings.proxy_mode}
          onChange={(e) =>
            void updateSettings({ ...settings, proxy_mode: e.target.value as ProxyMode })
          }
        >
          {PROXY_MODE_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {t(opt.label)}
            </option>
          ))}
        </select>
        <p className="settings-hint">
          {t(PROXY_MODE_OPTIONS.find((o) => o.value === settings.proxy_mode)?.hint ?? "")}
        </p>

        {settings.proxy_mode === "manual" && (
          <input
            className="settings-input"
            type="text"
            spellCheck={false}
            placeholder="127.0.0.1:7890"
            defaultValue={settings.proxy_url}
            onBlur={(e) => void updateSettings({ ...settings, proxy_url: e.target.value.trim() })}
          />
        )}

        {proxyStatus && (
          <p
            className={`proxy-status${proxyStatus.active ? " proxy-status-active" : ""}`}
            aria-live="polite"
          >
            {t(proxyStatus.description)}
          </p>
        )}
      </section>

      <section className="settings-section">
        <h3>{t("Startup")}</h3>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) => void updateSettings({ ...settings, autostart: e.target.checked })}
          />
          <span>{t("Launch automatically at Windows startup")}</span>
        </label>
      </section>

      <section className="settings-section">
        <div className="settings-section-heading">
          <h3>{t("Updates")}</h3>
          <span className="version-label">{currentVersion ? `v${currentVersion}` : t("Unknown")}</span>
        </div>

        <p className={`update-message update-message-${updateStatus}`} aria-live="polite">
          {t(updateMessage.key, updateMessage.vars)}
        </p>

        {availableUpdate?.body && updateStatus === "available" && (
          <p className="update-notes">{availableUpdate.body}</p>
        )}

        {updateStatus === "downloading" && (
          <div className="update-progress">
            <div className="update-progress-track">
              <div
                className={`update-progress-fill${progressPercent === null ? " update-progress-indeterminate" : ""}`}
                style={progressPercent === null ? undefined : { width: `${progressPercent}%` }}
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
          onClick={() => void (updateStatus === "available" ? installUpdate() : checkForUpdates())}
        >
          {updateStatus === "checking"
            ? t("Checking...")
            : updateStatus === "downloading"
              ? t("Downloading...")
              : updateStatus === "restarting"
                ? t("Restarting...")
                : updateStatus === "available"
                  ? t("Install v{v}", { v: availableUpdate?.version ?? "" })
                  : t("Check for updates")}
        </button>
      </section>
    </div>
  );
}

export default Settings;
