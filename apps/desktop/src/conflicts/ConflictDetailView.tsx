import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import { formatAge, type CardModel } from "@/fleet/fleet-model";
import { useUiStore } from "@/state/ui-store";
import { Button } from "@/ui";
import { BaseNotice } from "./ConflictPanel";
import { focusOpener } from "./focus";
import { LimitsLine } from "./LimitsPopover";
import { pairDetailView, REASON_KEY, sideName } from "./prediction-model";

export interface ConflictDetailViewProps {
  fleet: RepoFleet;
  cards: readonly CardModel[];
  pairId: string;
  nowMs: number;
}

const h3 = "m-0 text-text-main [font:var(--gr-type-body-strong)]";
const muted = "m-0 text-text-muted [font:var(--gr-type-secondary)]";

/**
 * The detail of one pair in the centre (D7): files with the lines of each side. Paths and ranges are text only. The
 * focus goes to the heading on open; Esc (when no popover of this view is open) and "Back" return to the main view
 * and give the focus back to the band's "View" button (D18).
 */
export function ConflictDetailView({ fleet, cards, pairId, nowMs }: ConflictDetailViewProps) {
  const { t } = useTranslation();
  const backToMain = useUiStore((s) => s.backToMain);
  const heading = useRef<HTMLHeadingElement>(null);
  const [popoverOpen, setPopoverOpen] = useState(false);
  const { repoId } = fleet.repo;
  const pair = fleet.repo.pairs.find((p) => p.pairId === pairId);
  const view = pairDetailView(fleet, cards, pairId, nowMs, t);

  useEffect(() => {
    heading.current?.focus();
  }, []);

  const back = () => {
    backToMain();
    focusOpener(repoId, pairId);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Escape" || event.defaultPrevented || popoverOpen) return;
    event.preventDefault();
    back();
  };

  const glyph = /^([⚡⚠])\s*/.exec(view.title);
  const title = glyph ? view.title.slice(glyph[0].length) : view.title;
  const a = pair ? sideName(fleet, cards, pair.a, t) : "";
  const b = pair ? sideName(fleet, cards, pair.b, t) : "";
  const age = pair?.computedAtUtcMs != null ? formatAge(nowMs - pair.computedAtUtcMs, t) : "";
  const stale = view.status === "stale";
  const withFiles = view.status === "default" || view.status === "against-base" || view.status === "overlap-only" || stale;

  let body;
  if (view.status === "gone") body = <p className={muted}>{t("conflicts.detail.gone")}</p>;
  else if (view.status === "calculating") body = <p role="status" className={muted}>{t("conflicts.detail.calculating")}</p>;
  else if (view.status === "resolved") body = <p className={muted}>{t("conflicts.detail.resolved", { a, b, age })}</p>;
  else if (view.status === "pending") {
    body = (
      <>
        {pair?.reason ? <p className={muted}>{t(REASON_KEY[pair.reason])}</p> : null}
        {view.basePending !== null ? <BaseNotice pending={view.basePending} /> : null}
      </>
    );
  } else if (view.status === "loading") body = <p role="status" className={muted}>{t("conflicts.detail.loading")}</p>;
  else {
    body = (
      <div className="flex flex-col gap-(--gr-space-4) @min-[640px]:flex-row">
        <div className="flex min-w-0 flex-1 flex-col gap-(--gr-space-3)">
          {stale ? <p role="status" className={muted}>{view.freshnessText}</p> : null}
          {view.conflictFiles.length > 0 ? (
            <section className="flex flex-col gap-(--gr-space-2)">
              <h3 className={h3}>{t("conflicts.detail.conflict")}</h3>
              <p className={muted}>{t("conflicts.detail.conflictHint")}</p>
              <ul className="m-0 flex list-none flex-col gap-(--gr-space-2) p-0">
                {view.conflictFiles.map((f) => (
                  <li key={f.path} className="flex flex-col">
                    <code className="[overflow-wrap:anywhere] text-text-main [font:var(--gr-type-mono)]">{f.path}</code>
                    <span className={stale ? "text-text-muted" : "text-text-main"}>{f.kindText}</span>
                    <span className={stale ? "text-text-muted" : "text-text-main"}>{f.aText}</span>
                    <span className={stale ? "text-text-muted" : "text-text-main"}>{f.bText}</span>
                    {f.mergeRule ? <span className="text-warning-text">{t("conflicts.detail.mergeRule")}</span> : null}
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {view.overlapFiles.length > 0 ? (
            <section className="flex flex-col gap-(--gr-space-2)">
              <h3 className={h3}>{t("conflicts.detail.overlap")}</h3>
              <p className={muted}>{t("conflicts.detail.overlapHint")}</p>
              <ul className="m-0 flex list-none flex-col gap-(--gr-space-1) p-0">
                {view.overlapFiles.map((path) => (
                  <li key={path}>
                    <code className="[overflow-wrap:anywhere] [font:var(--gr-type-mono)]">{path}</code>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {view.truncated ? <p className={muted}>{t("conflicts.detail.truncated")}</p> : null}
        </div>
        <section className="flex min-w-0 flex-col gap-(--gr-space-2) @min-[640px]:w-1/3">
          <h3 className={h3}>{t("conflicts.detail.todoTitle")}</h3>
          <p className="m-0 [font:var(--gr-type-body)]">{t("conflicts.detail.todo")}</p>
          <p className={muted}>{t("conflicts.detail.prediction")}</p>
          <LimitsLine limits={fleet.repo.limits} onOpenChange={setPopoverOpen} />
        </section>
      </div>
    );
  }

  return (
    <section onKeyDown={onKeyDown} className="@container flex min-h-0 flex-1 flex-col gap-(--gr-space-3) overflow-auto">
      <div>
        <Button variant="link" size="sm" onClick={back}>
          {t("conflicts.detail.back")}
        </Button>
      </div>
      <div className="flex flex-col gap-(--gr-space-1)">
        <h2 ref={heading} tabIndex={-1} className="m-0 text-text-main [font:var(--gr-type-title)]">
          {view.status === "gone" ? (
            t("conflicts.panel.title")
          ) : (
            <>
              {glyph ? <span aria-hidden="true">{glyph[1]}</span> : null} {title}
            </>
          )}
        </h2>
        {view.status !== "gone" ? <p className={muted}>{view.subtitle}</p> : null}
      </div>
      {body}
      {!withFiles && view.status !== "gone" ? <LimitsLine limits={fleet.repo.limits} onOpenChange={setPopoverOpen} /> : null}
    </section>
  );
}
