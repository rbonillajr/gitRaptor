import { cx } from "./cx";

export type StatusPillTone = "success" | "warning" | "danger" | "muted";

const TONE: Record<StatusPillTone, string> = {
  success: "bg-success-bg text-success-text",
  warning: "bg-warning-bg text-warning-text",
  danger: "bg-danger-bg text-danger-text",
  muted: "bg-surface-control text-text-muted",
};

export interface StatusPillProps {
  /** A single glyph (`●`, `✖`…); decorative, the label always says the same. */
  symbol: string;
  tone: StatusPillTone;
  /** Already translated; always visible (the colour and the glyph only repeat it). */
  label: string;
  className?: string;
}

/**
 * A polite live region that says in words, with a glyph and a tint, how a connection is doing. Never clicked.
 * Pure: it knows nothing of the engine; the caller picks symbol, tone and text.
 */
export function StatusPill({ symbol, tone, label, className }: StatusPillProps) {
  return (
    <span
      role="status"
      aria-live="polite"
      data-tone={tone}
      className={cx(
        "inline-flex h-(--gr-space-5) items-center gap-(--gr-space-1) rounded-pill px-(--gr-space-2) [font:var(--gr-type-caption)]",
        TONE[tone],
        className,
      )}
    >
      <span aria-hidden="true">{symbol}</span>
      {label}
    </span>
  );
}
