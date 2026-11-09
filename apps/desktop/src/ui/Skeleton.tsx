import { useTranslation } from "react-i18next";
import { cx } from "./cx";

export interface SkeletonProps {
  /** How many placeholder bars; default 3. */
  lines?: number;
  className?: string;
}

/**
 * A loading placeholder: static bars (the app has no keyframes) and a screen-reader-only "Loading…".
 * It never stands for "empty": show it only while the data has not arrived.
 */
export function Skeleton({ lines = 3, className }: SkeletonProps) {
  const { t } = useTranslation();
  return (
    <div role="status" aria-busy="true" className={cx("flex flex-col gap-(--gr-space-2)", className)}>
      <span className="sr-only">{t("ui.common.loading")}</span>
      {Array.from({ length: lines }, (_, i) => (
        <span key={i} aria-hidden="true" className="block h-(--gr-space-3) w-full rounded-md bg-surface-hover" />
      ))}
    </div>
  );
}
