import { useState } from "react";
import type { UsageMetric, UsageSnapshot } from "../types";
import { translateLabel, type Lang, type T } from "../i18n";
import {
  agoText,
  formatAmount,
  paceNumber,
  resetClock,
  runOut,
  timeText,
  untilText,
  usedPercent,
  windowPace,
} from "../utils";
import { ProgressBar } from "./ProgressBar";
import { Ring } from "./Ring";
import { RefreshIcon, WarnIcon } from "./Icons";

/** Up to this many percentage windows are drawn as rings; more as bars. */
const RING_MAX = 4;
/** With this many rings or fewer, rings sit beside the text. */
const RINGS_BESIDE_MAX = 3;

interface Props {
  snapshot: UsageSnapshot;
  showRemaining: boolean;
  onToggleMode: () => void;
  onRefresh: (provider: string) => Promise<void>;
  t: T;
  lang: Lang;
  now: number;
}

interface Ctx {
  t: T;
  lang: Lang;
  now: number;
  showRemaining: boolean;
}

function isExpired(m: UsageMetric, now: number): boolean {
  return m.reset_at !== null && m.reset_at * 1000 <= now;
}

/** "Resets 14:26", "Not used yet", or the amount for non-percent units. */
function whenText(m: UsageMetric, { t, lang, now }: Ctx): string {
  const parts: string[] = [];
  if (m.unit !== "percent" && m.limit !== null) {
    parts.push(`${formatAmount(m.used, m.unit)} / ${formatAmount(m.limit, m.unit)}`);
  }
  if (m.reset_at !== null) {
    parts.push(
      isExpired(m, now)
        ? t("Reset time passed")
        : t("Resets {when}", { when: resetClock(m.reset_at, lang, t, new Date(now)) }),
    );
  } else if ((usedPercent(m) ?? 0) === 0 && m.unit === "percent") {
    parts.push(t("Not used yet"));
  }
  return parts.join(" · ");
}

function paceTitle(delta: number, t: T): string {
  const n = `${Math.abs(delta)}%`;
  if (delta > 0)
    return t(
      "{n} more used than an even pace would have by now — at this rate it runs out before it resets",
      { n },
    );
  if (delta < 0)
    return t(
      "{n} less used than an even pace would have by now — at this rate it lasts until it resets",
      { n },
    );
  return t("Right on an even pace");
}

function metricTitle(m: UsageMetric, ctx: Ctx): string {
  const { t, lang, now, showRemaining } = ctx;
  const used = usedPercent(m) ?? 0;
  const lines = [
    `${translateLabel(t, m.label)} · ${t(showRemaining ? "{n} left" : "{n} used", {
      n: `${Math.round(showRemaining ? 100 - used : used)}%`,
    })}`,
  ];
  if (m.unit !== "percent" && m.limit !== null) {
    lines.push(`${formatAmount(m.used, m.unit)} / ${formatAmount(m.limit, m.unit)}`);
  }
  if (m.reset_at !== null) {
    lines.push(
      `${t("Resets {when}", { when: timeText(m.reset_at, lang, now) })} · ${untilText(m.reset_at, t, now)}`,
    );
  }
  const pace = windowPace(m, now);
  if (pace) lines.push(`${t("Pace {n}", { n: paceNumber(pace.delta) })} — ${paceTitle(pace.delta, t)}`);
  const out = runOut(m, now);
  if (out) {
    lines.push(
      out.lasts
        ? t("Lasts past the reset at this pace")
        : t("Runs out ~{when} at this pace", { when: timeText(out.at, lang, now) }),
    );
  }
  lines.push("", t(showRemaining ? "Show how much is used" : "Show how much is left"));
  return lines.join("\n");
}

function PaceChip({ m, ctx }: { m: UsageMetric; ctx: Ctx }) {
  const pace = windowPace(m, ctx.now);
  if (!pace) return null;
  // Like the rings, the sign follows what is shown: in "left" mode being
  // ahead of pace reads as negative. The colour always follows overuse.
  const shown = ctx.showRemaining ? -pace.delta : pace.delta;
  return (
    <span
      className={`pace ${pace.over ? "over" : pace.delta < 0 ? "under" : ""}`}
      title={`${ctx.t("Pace {n}", { n: paceNumber(shown) })}\n${paceTitle(pace.delta, ctx.t)}`}
    >
      {ctx.t("Pace {n}", { n: paceNumber(shown) })}
    </span>
  );
}

function WindowLine({ m, ctx }: { m: UsageMetric; ctx: Ctx }) {
  const text = whenText(m, ctx);
  const until = m.reset_at !== null && !isExpired(m, ctx.now) ? untilText(m.reset_at, ctx.t, ctx.now) : "";
  return (
    <div className="window-line">
      <span className="wl-label">{translateLabel(ctx.t, m.label)}</span>
      <span className="wl-when" title={until ? `${text} · ${until}` : text}>
        {text}
      </span>
      <PaceChip m={m} ctx={ctx} />
    </div>
  );
}

function BarRow({ m, ctx }: { m: UsageMetric; ctx: Ctx }) {
  const used = usedPercent(m) ?? 0;
  const shown = Math.round(ctx.showRemaining ? 100 - used : used);
  return (
    <div className="bar-row" title={metricTitle(m, ctx)}>
      <span className="bar-label">{translateLabel(ctx.t, m.label)}</span>
      <ProgressBar used={used} showRemaining={ctx.showRemaining} />
      <span className="bar-value">{shown}%</span>
      <span className="bar-reset">
        {m.reset_at !== null && !isExpired(m, ctx.now) ? untilText(m.reset_at, ctx.t, ctx.now) : ""}
      </span>
    </div>
  );
}

function ValueLine({ m, ctx }: { m: UsageMetric; ctx: Ctx }) {
  const value =
    m.limit !== null
      ? `${formatAmount(m.used, m.unit)} / ${formatAmount(m.limit, m.unit)}`
      : ctx.t("{amount} remaining", { amount: formatAmount(m.used, m.unit) });
  return (
    <div className="value-line">
      <span className="wl-label">{translateLabel(ctx.t, m.label)}</span>
      <span className="value-amount">{value}</span>
    </div>
  );
}

export function ProviderCard({ snapshot, showRemaining, onToggleMode, onRefresh, t, lang, now }: Props) {
  const [busy, setBusy] = useState(false);
  const { display_name, status, message, metrics, plan, account, updated_at } = snapshot;
  const ctx: Ctx = { t, lang, now, showRemaining };
  // Last-known numbers carried through a transient failure (the backend keeps
  // them for a while): shown as real data, dated "As of".
  const stale = snapshot.stale && metrics.length > 0;
  // Whether there are numbers to draw: fresh, carried over, or the partial
  // figures a plan without a usage API may still report.
  const showMetrics = metrics.length > 0 && (status === "ok" || stale || status === "no_quota_api");
  const showIdentity = status === "ok" || stale || status === "no_quota_api";

  async function refresh() {
    if (busy) return;
    setBusy(true);
    try {
      await onRefresh(snapshot.provider);
    } finally {
      setBusy(false);
    }
  }

  const gauges = metrics.filter((m) => usedPercent(m) !== null);
  const values = metrics.filter((m) => usedPercent(m) === null);
  const asRings = gauges.length > 0 && gauges.length <= RING_MAX;
  const beside = asRings && gauges.length <= RINGS_BESIDE_MAX;
  const expired = stale && gauges.some((m) => isExpired(m, now));

  // The window that runs out soonest ahead of its reset, if any.
  const warning = gauges
    .map((m) => ({ m, out: runOut(m, now) }))
    .filter((x): x is { m: UsageMetric; out: { lasts: false; at: number } } => !!x.out && !x.out.lasts)
    .sort((a, b) => a.out.at - b.out.at)[0];

  const ageTitle = stale
    ? t("Showing the last reading; the latest refresh failed: {why}", { why: t(message ?? "") })
    : t("Updated {when}", { when: timeText(updated_at, lang, now) });

  const rings = asRings && (
    <div className={`rings${beside ? " beside" : ""}${beside && gauges.length > 2 ? " compact" : ""}`}>
      {gauges.map((m) => (
        <Ring
          key={m.label}
          used={usedPercent(m) ?? 0}
          showRemaining={showRemaining}
          label={translateLabel(t, m.label)}
          title={metricTitle(m, ctx)}
          dim={stale && isExpired(m, now)}
          onClick={onToggleMode}
        />
      ))}
    </div>
  );

  return (
    <div className={`card card-${stale ? "ok card-stale" : status}`}>
      <div className="card-header">
        <span className="provider-name">{display_name}</span>
        {plan && showIdentity && <span className="plan-pill">{plan}</span>}
        <span className="card-spacer" />
        {(status === "ok" || stale) && (
          <span className={`card-age${stale ? " stale" : ""}`} title={ageTitle}>
            {stale ? t("As of {when}", { when: timeText(updated_at, lang, now) }) : agoText(updated_at, t, now)}
          </span>
        )}
        {status === "error" && !stale && <span className="badge badge-error">{t("Error")}</span>}
        {status === "no_quota_api" && <span className="badge badge-muted">{t("No usage API")}</span>}
        <button
          type="button"
          className={`card-refresh${busy ? " busy" : ""}${status === "error" && !stale ? " shown" : ""}`}
          onClick={refresh}
          disabled={busy}
          title={t("Refresh this card")}
          aria-label={t("Refresh this card")}
        >
          <RefreshIcon spinning={busy} size={12} />
        </button>
      </div>

      {account && showIdentity && (
        <div className="card-account" title={account}>
          {account}
        </div>
      )}

      {showMetrics && (
        <div className={`card-body${beside ? " with-rings" : ""}`}>
          {!beside && rings}
          <div className="window-lines">
            {asRings
              ? gauges.map((m) => <WindowLine key={m.label} m={m} ctx={ctx} />)
              : gauges.map((m) => <BarRow key={m.label} m={m} ctx={ctx} />)}
            {values.map((m) => (
              <ValueLine key={m.label} m={m} ctx={ctx} />
            ))}
          </div>
          {beside && rings}
        </div>
      )}

      {status === "ok" && warning && (
        <div className="card-note warn">
          <WarnIcon />
          <span>
            {t("{label} runs out ~{when} at this pace", {
              label: translateLabel(t, warning.m.label),
              when: timeText(warning.out.at, lang, now),
            })}
          </span>
        </div>
      )}

      {expired && (
        <div className="card-note muted">
          {t("A window has passed its reset time; the current allowance is unknown")}
        </div>
      )}

      {status !== "ok" && !stale && message && <p className="card-message">{t(message)}</p>}
    </div>
  );
}
