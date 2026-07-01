export function formatResetIn(resetAt: number): string {
  const now = Date.now() / 1000;
  const diff = resetAt - now;
  if (diff <= 0) return "即将重置";
  const totalMinutes = Math.floor(diff / 60);
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor((totalMinutes % 1440) / 60);
  const minutes = totalMinutes % 60;
  if (days > 0) return `${days} 天后重置`;
  if (hours > 0) return `${hours} 小时 ${minutes} 分后重置`;
  return `${minutes} 分钟后重置`;
}

export function formatUpdatedAt(ts: number): string {
  const d = new Date(ts * 1000);
  return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
}

export function formatAmount(value: number, unit: string): string {
  if (unit === "usd") return `$${value.toFixed(2)}`;
  if (unit === "percent") return `${value.toFixed(0)}%`;
  if (unit === "tokens") return formatCompactNumber(value);
  return `${Math.round(value)}`;
}

function formatCompactNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return `${Math.round(value)}`;
}
