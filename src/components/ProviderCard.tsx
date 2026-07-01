import type { UsageSnapshot } from "../types";
import { ProgressBar } from "./ProgressBar";
import { formatAmount, formatResetIn } from "../utils";

interface Props {
  snapshot: UsageSnapshot;
}

export function ProviderCard({ snapshot }: Props) {
  const { display_name, status, message, metrics } = snapshot;

  return (
    <div className={`card card-${status}`}>
      <div className="card-header">
        <span className="provider-name">{display_name}</span>
        {status !== "ok" && (
          <span className={`badge badge-${status}`}>
            {status === "not_connected" ? "Not connected" : "Error"}
          </span>
        )}
      </div>

      {status === "ok" && metrics.length > 0 && (
        <div className="metric-list">
          {metrics.map((m) => (
            <div className="metric-row" key={m.label}>
              <div className="metric-top">
                <span className="metric-label">{m.label}</span>
                <span className="metric-value">
                  {m.limit !== null
                    ? `${formatAmount(m.used, m.unit)} / ${formatAmount(m.limit, m.unit)}`
                    : `${formatAmount(m.used, m.unit)} remaining`}
                </span>
              </div>
              {(m.percent !== null || m.reset_at) && (
                <div className="metric-bottom">
                  {m.percent !== null && <ProgressBar percent={m.percent} />}
                  {m.reset_at && <span className="metric-reset">{formatResetIn(m.reset_at)}</span>}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {status !== "ok" && message && <p className="card-message">{message}</p>}
    </div>
  );
}
