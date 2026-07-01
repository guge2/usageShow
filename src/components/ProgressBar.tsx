interface Props {
  percent: number;
}

function colorFor(percent: number): string {
  if (percent >= 90) return "var(--danger)";
  if (percent >= 70) return "var(--warning)";
  return "var(--accent)";
}

export function ProgressBar({ percent }: Props) {
  const clamped = Math.min(100, Math.max(0, percent));
  return (
    <div className="progress-track">
      <div
        className="progress-fill"
        style={{ width: `${clamped}%`, backgroundColor: colorFor(clamped) }}
      />
    </div>
  );
}
