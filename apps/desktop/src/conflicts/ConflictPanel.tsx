import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";
import { useUiStore } from "@/state/ui-store";
import { Button, ConflictBand, CopyCommand } from "@/ui";
import { cx } from "@/ui/cx";
import { PANEL_TITLE_ID, registerOpener } from "./focus";
import { LimitsLine } from "./LimitsPopover";
import { type BasePending } from "./prediction-model";
import { usePredictionView } from "./usePredictionView";

export interface ConflictPanelProps {
  fleet: RepoFleet;
  cards: readonly CardModel[];
  nowMs: number;
}

const MAX_BANDS = 3;

/** D11: the pending base warning with the confirm command the backend built, to run in the user's own terminal. */
export function BaseNotice({ pending }: { pending: BasePending }) {
  const { t } = useTranslation();
  const { command } = pending;
  return (
    <div className="flex flex-col gap-(--gr-space-1)">
      <p className="m-0 text-warning-text [font:var(--gr-type-body-strong)]">{t("conflicts.base.pending", { base: pending.base })}</p>
      {command.state === "offered" ? (
        <>
          <CopyCommand command={command.command} />
          <p className="m-0 text-text-muted [font:var(--gr-type-secondary)]">
            {command.withPath ? t("conflicts.base.hint") : t("conflicts.base.hintFolder", { repo: pending.repo })}
          </p>
        </>
      ) : (
        <p className="m-0 text-text-muted [font:var(--gr-type-secondary)]">{t("conflicts.base.raptorUnknown")}</p>
      )}
    </div>
  );
}

/**
 * The predicted conflicts of the open repo, above the main view of the centre (D5): bands (⚡ first, at most 3
 * until expanded), freshness lines, the pending base warning and the limits line. Reads the model from the cache
 * entry it is given and opens a pair through the ui store.
 */
export function ConflictPanel({ fleet, cards, nowMs }: ConflictPanelProps) {
  const { t } = useTranslation();
  const view = usePredictionView(fleet, cards, nowMs);
  const openPairDetail = useUiStore((s) => s.openPairDetail);
  const [expanded, setExpanded] = useState(false);
  const listId = useId();
  const { repoId, name } = fleet.repo;
  const shown = expanded ? view.bands : view.bands.slice(0, MAX_BANDS);
  const hidden = view.bands.length - MAX_BANDS;
  const quiet = view.bands.length === 0 && view.basePending === null;

  return (
    <section aria-labelledby={PANEL_TITLE_ID} className="flex flex-col gap-(--gr-space-2)">
      <h2 id={PANEL_TITLE_ID} tabIndex={-1} className={cx("m-0 text-text-main [font:var(--gr-type-body-strong)]", quiet && "sr-only")}>
        {t("conflicts.panel.title")}
      </h2>
      {view.basePending !== null ? <BaseNotice pending={view.basePending} /> : null}
      {view.bands.length > 0 ? (
        <ul id={listId} aria-label={t("conflicts.panel.list", { repo: name })} className="m-0 flex list-none flex-col gap-(--gr-space-2) p-0">
          {shown.map((b) => (
            <li key={b.pairId}>
              <ConflictBand
                variant="row"
                level={b.level}
                aLabel={b.aLabel}
                bLabel={b.bLabel}
                firstFile={b.firstFile}
                moreText={b.moreText}
                freshnessText={b.freshnessText}
                stale={b.stale}
                isNew={b.isNew}
                a11yLabel={b.a11yLabel}
                openRef={registerOpener(repoId, b.pairId)}
                onOpen={() => openPairDetail({ repoId, pairId: b.pairId })}
              />
            </li>
          ))}
        </ul>
      ) : null}
      {hidden > 0 ? (
        <Button variant="link" size="sm" className="self-start" aria-expanded={expanded} aria-controls={listId} onClick={() => setExpanded(!expanded)}>
          {expanded ? t("conflicts.panel.less") : t("conflicts.panel.more", { count: hidden })}
        </Button>
      ) : null}
      {view.lines.map((line) => (
        <p key={line} role="status" className="m-0 text-text-muted [font:var(--gr-type-secondary)]">
          {line}
        </p>
      ))}
      <LimitsLine limits={view.limits} />
    </section>
  );
}
