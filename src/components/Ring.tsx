import type { CSSProperties } from "react";
import { severity } from "../utils";

interface Props {
  /** 0-100, already used. Severity colour always follows usage. */
  used: number;
  /** Show what's left instead of what's used. */
  showRemaining: boolean;
  label: string;
  title: string;
  dim?: boolean;
  onClick?: () => void;
}

export function Ring({ used, showRemaining, label, title, dim, onClick }: Props) {
  const fill = Math.round(showRemaining ? 100 - used : used);
  return (
    <button
      type="button"
      className={`ring sev-${severity(used)}${dim ? " dim" : ""}`}
      title={title}
      onClick={onClick}
    >
      <span className="ring-dial" style={{ "--p": fill } as CSSProperties}>
        <b>{fill}%</b>
      </span>
      <span className="ring-label">{label}</span>
    </button>
  );
}
