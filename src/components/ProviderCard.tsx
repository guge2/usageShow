import type { UsageSnapshot, UsageStatus } from "../types";
import { ProgressBar } from "./ProgressBar";
import { formatAgo, formatAmount, formatResetIn } from "../utils";

interface Props {
  snapshot: UsageSnapshot;
}

const STATUS_LABEL: Record<Exclude<UsageStatus, "ok">, string> = {
  not_connected: "Not connected",
  // Signed in and working — this plan simply has no usage endpoint, so it is
  // deliberately not styled or worded as a failure.
  no_quota_api: "No usage API",
  error: "Error",
};

export function ProviderCard({ snapshot }: Props) {
  const { display_name, status, message, metrics, stale, updated_at } = snapshot;
  const hasMetrics = metrics.length > 0;
  // Showing last-known numbers through a transient failure: label the card as
  // stale rather than as broken, and say how old the numbers are.
  const showingStaleData = stale && hasMetrics;

  return (
    <div className={`card card-${showingStaleData ? "stale" : status}`}>
      <div className="card-header">
        <span className="provider-name">{display_name}</span>
        {status !== "ok" &&
          (showingStaleData ? (
            <span className="badge badge-stale" title={message ?? undefined}>
              {formatAgo(updated_at)}
            </span>
          ) : (
            <span className={`badge badge-${status}`}>{STATUS_LABEL[status]}</span>
          ))}
      </div>

      {hasMetrics && (
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
                  {m.reset_at && (
                    <span className="metric-reset">{formatResetIn(m.reset_at)}</span>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {status !== "ok" && message && !showingStaleData && (
        <p className="card-message">{message}</p>
      )}
    </div>
  );
}
