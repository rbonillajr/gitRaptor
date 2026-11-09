import { useTranslation } from "react-i18next";
import { cx } from "./cx";

export type StatusTone = "active" | "inactive" | "conflict" | "live";

/** The shape tells the state apart without colour (WCAG 1.4.1): disc, ring, diamond. */
const TONE: Record<StatusTone, string> = {
  active: "rounded-pill bg-success",
  inactive: "rounded-pill border-(length:--gr-size-lane-stroke) border-text-muted",
  conflict: "bg-danger [clip-path:polygon(50%_0,100%_50%,50%_100%,0_50%)]",
  live: "rounded-pill bg-live",
};

/** A non-interactive state marker. Use `decorative` when the adjacent text already names the state. */
export function StatusDot({ tone, decorative = false, className }: { tone: StatusTone; decorative?: boolean; className?: string }) {
  const { t } = useTranslation();
  const name = t(`ui.status.${tone}`);
  return (
    <span
      {...(decorative ? { "aria-hidden": true } : { role: "img", "aria-label": name })}
      className={cx("inline-block size-(--gr-space-2) shrink-0", TONE[tone], className)}
    />
  );
}
