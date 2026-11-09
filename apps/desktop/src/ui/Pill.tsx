import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "./cx";
import { CircleAlert, OctagonX, Undo } from "./icons";
import { StatusDot } from "./StatusDot";

export type PillKind = "reversible" | "confirm" | "irreversible" | "live";

const KIND: Record<PillKind, { bg: string; glyph: ReactNode }> = {
  reversible: { bg: "bg-success-bg", glyph: <span className="inline-flex text-success-text"><Undo size="sm" /></span> },
  confirm: { bg: "bg-warning-bg", glyph: <span className="inline-flex text-warning-text"><CircleAlert size="sm" /></span> },
  irreversible: { bg: "bg-danger-bg", glyph: <span className="inline-flex text-danger-text"><OctagonX size="sm" /></span> },
  live: { bg: "bg-live-bg", glyph: <StatusDot tone="live" decorative /> },
};

/** Informs, never clicked. The label is always visible; the glyph and the tint repeat it. */
export function Pill({ kind, announce = false, className }: { kind: PillKind; announce?: boolean; className?: string }) {
  const { t } = useTranslation();
  const { bg, glyph } = KIND[kind];
  return (
    <span
      {...(announce ? { role: "status" } : {})}
      className={cx(
        "inline-flex h-(--gr-space-5) items-center gap-(--gr-space-1) rounded-pill px-(--gr-space-2) text-text-main [font:var(--gr-type-caption)]",
        bg,
        className,
      )}
    >
      {glyph}
      {t(`ui.pill.${kind}`)}
    </span>
  );
}
