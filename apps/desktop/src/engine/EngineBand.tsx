import { useTranslation } from "react-i18next";
import { Button, CopyCommand } from "@/ui";
import { cx } from "@/ui/cx";
import type { EngineView } from "./engine-view";

/** The literal the user runs while the app cannot launch the engine itself (the launcher is a separate dependency). */
export const START_COMMAND = "raptor daemon start";

export interface EngineBandProps {
  band: NonNullable<EngineView["band"]>;
  /** `start` is only ever `false` for now: the app cannot start the engine, so it only asks to retry the connection. */
  onRetry: (start: boolean) => void;
  /** Buttons stay `aria-disabled` until the next connection state arrives. */
  pending?: boolean;
}

/**
 * The strip over the centre: what is wrong or in progress, since when, and the one thing to do. A status
 * region, so a screen reader hears it when it appears. Pure: callbacks in, no IPC and no store.
 */
export function EngineBand({ band, onRetry, pending = false }: EngineBandProps) {
  const { t } = useTranslation();
  const tint = band.action === "start" ? "bg-danger-bg" : "bg-warning-bg";
  return (
    <div
      role="status"
      data-band={band.key}
      className={cx(
        "flex flex-wrap items-center gap-(--gr-space-2) border-b border-border-subtle px-(--gr-space-4) py-(--gr-space-2) text-text-main [font:var(--gr-type-secondary)]",
        tint,
      )}
    >
      <span>
        {t(band.key, band.values)}
        {band.detailKey ? <> {t(band.detailKey)}</> : null}
      </span>
      {band.action ? (
        <Button size="sm" disabled={pending} onClick={() => onRetry(false)}>
          {t("engine.action.retryNow")}
        </Button>
      ) : null}
      {band.action === "start" ? <CopyCommand command={START_COMMAND} /> : null}
    </div>
  );
}
