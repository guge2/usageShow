import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "./api";
import { ALL_PROVIDERS, REFRESH_INTERVAL_OPTIONS } from "./types";
import type { AppSettings } from "./types";
import "./App.css";
import "./Settings.css";

function Settings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);

  useEffect(() => {
    getSettings().then(setSettings);
  }, []);

  function update(next: AppSettings) {
    setSettings(next);
    void saveSettings(next);
  }

  function toggleProvider(id: string) {
    if (!settings) return;
    const enabled = settings.enabled_providers.includes(id)
      ? settings.enabled_providers.filter((p) => p !== id)
      : [...settings.enabled_providers, id];
    update({ ...settings, enabled_providers: enabled });
  }

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
            update({ ...settings, refresh_interval_secs: Number(e.target.value) })
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
          {ALL_PROVIDERS.map((p) => (
            <label key={p.id} className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.enabled_providers.includes(p.id)}
                onChange={() => toggleProvider(p.id)}
              />
              <span>{p.label}</span>
            </label>
          ))}
        </div>
      </section>

      <section className="settings-section">
        <h3>Startup</h3>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) => update({ ...settings, autostart: e.target.checked })}
          />
          <span>Launch automatically at Windows startup</span>
        </label>
      </section>
    </div>
  );
}

export default Settings;
