import { severity } from "../utils";

interface Props {
  /** 0-100, already used. */
  used: number;
  showRemaining: boolean;
}

export function ProgressBar({ used, showRemaining }: Props) {
  const clamped = Math.min(100, Math.max(0, used));
  const width = showRemaining ? 100 - clamped : clamped;
  return (
    <div className="progress-track">
      <div className={`progress-fill sev-${severity(clamped)}`} style={{ width: `${width}%` }} />
    </div>
  );
}
