import type { Ref } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";
import { cx } from "./cx";

export interface ConflictBandProps {
  variant: "row" | "badge";
  level: "conflict" | "overlap";
  aLabel: string;
  bLabel: string;
  firstFile: string | null;
  moreText: string | null;
  freshnessText: string;
  stale: boolean;
  isNew: boolean;
  /** Already translated, without glyphs: the accessible name of the band (D19). */
  a11yLabel: string;
  onOpen?: () => void;
  openRef?: Ref<HTMLButtonElement>;
}

/**
 * A predicted conflict (⚡) or an overlap (⚠) between two sides. Level, staleness and "new" are said in text, never
 * only in colour; the glyph is decorative. `badge` has no button: the lane that draws it activates it (D6, D19).
 */
export function ConflictBand({
  variant,
  level,
  aLabel,
  bLabel,
  firstFile,
  moreText,
  freshnessText,
  stale,
  isNew,
  a11yLabel,
  onOpen,
  openRef,
}: ConflictBandProps) {
  const { t } = useTranslation();
  const conflict = level === "conflict";
  const glyph = (
    <span aria-hidden="true" className={conflict ? "text-danger-text" : "text-warning-text"}>
      {conflict ? "⚡" : "⚠"}
    </span>
  );
  const file = firstFile !== null ? <code className="min-w-0 [overflow-wrap:anywhere] [font:var(--gr-type-mono)]">{firstFile}</code> : null;
  const freshness = <span className={stale ? "text-text-muted" : undefined}>{freshnessText}</span>;

  if (variant === "badge") {
    return (
      <span
        role="group"
        aria-label={a11yLabel}
        className={cx(
          "inline-flex max-w-full items-center gap-(--gr-space-1) rounded-pill bg-surface-control px-(--gr-space-2) text-text-main [font:var(--gr-type-caption)]",
          stale && "text-text-muted",
        )}
      >
        {glyph}
        <span className="min-w-0 truncate">
          {aLabel} ↔ {bLabel}
        </span>
        {file}
        {freshness}
      </span>
    );
  }

  return (
    <div
      role="group"
      aria-label={a11yLabel}
      className={cx(
        "flex flex-wrap items-center gap-x-(--gr-space-2) gap-y-(--gr-space-1) border-l-2 pl-(--gr-space-2) text-text-main [font:var(--gr-type-body)]",
        conflict ? "border-danger" : "border-warning",
        stale && "border-dashed",
      )}
    >
      {glyph}
      <span className={conflict ? "text-danger-text" : "text-warning-text"}>{t(conflict ? "conflicts.a11y.conflict" : "conflicts.a11y.overlap")}</span>
      <span className="min-w-0 [overflow-wrap:anywhere]">
        {aLabel} ↔ {bLabel}
      </span>
      {file}
      {moreText !== null ? <span className="text-text-muted">{moreText}</span> : null}
      <span className="text-text-muted">{freshness}</span>
      {isNew ? <span className="rounded-pill bg-danger-bg px-(--gr-space-2) text-danger-text [font:var(--gr-type-caption)]">{t("conflicts.band.new")}</span> : null}
      <Button
        size="sm"
        className="ml-auto"
        aria-label={t("conflicts.band.openLabel", { a: aLabel, b: bLabel })}
        {...(openRef ? { ref: openRef } : {})}
        {...(onOpen ? { onClick: onOpen } : {})}
      >
        {t("conflicts.band.open")}
      </Button>
    </div>
  );
}
