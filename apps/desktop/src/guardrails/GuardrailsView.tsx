import { skipToken, useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { engineKeys } from "@/client/queries";
import { formatAge } from "@/fleet/fleet-model";
import { useNow } from "@/fleet/useNow";
import type { GuardBlocks } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { Button, Skeleton, StatePanel } from "@/ui";
import { blockRows, type BlockRow, type CopiedException } from "./blocks-model";
import { PendingException } from "./PendingException";

export interface GuardrailsViewProps {
  repoName: string;
  live: boolean;
  onBack: () => void;
  onOpenRow: (row: BlockRow) => void;
  copied: CopiedException;
}

const muted = "m-0 text-text-muted [font:var(--gr-type-secondary)]";
const DAY_MS = 86_400_000;

function formatMoment(ms: number, lang: string): string {
  return new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(ms);
}

/** One line of the list: the time, who and where, what was tried and the rule that stopped it. Plain text only. */
function RowBody({ row, actor }: { row: BlockRow; actor: string }) {
  return (
    <span className="flex min-w-0 flex-col gap-(--gr-space-1)">
      <span className="text-text-main [font:var(--gr-type-body-strong)]">
        {row.kind === "denial" ? <span aria-hidden="true">⛔ </span> : null}
        {row.title}
      </span>
      <span className={muted}>
        {actor} · {row.attempted}
      </span>
      {row.stoppedBy.length > 0 ? <span className={muted}>{row.stoppedBy.join("; ")}</span> : null}
    </span>
  );
}

/**
 * The Guardrails center view (D11, D13): what GitRaptor stopped in the open repo, newest first, read from the cache
 * entry that `useGuardBlocks` keeps fresh. Never says "nothing blocked" before the first read or when the list is
 * unavailable. The engine's text enters only as React text. Esc goes back, as in the pair detail (D18).
 */
export function GuardrailsView({ repoName, live, onBack, onOpenRow, copied }: GuardrailsViewProps) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const repoId = useUiStore((s) => s.selectedRepoId);
  const data = useQuery<GuardBlocks>({ queryKey: engineKeys.guard(repoId ?? ""), queryFn: skipToken, enabled: repoId !== null }).data;
  const nowMs = useNow();
  const heading = useRef<HTMLHeadingElement>(null);
  const rows = useMemo(() => (data === undefined ? [] : blockRows(data, live, t, lang)), [data, live, t, lang]);
  const actors = useMemo(() => {
    const map = new Map<string, string>();
    for (const e of data?.entries ?? []) {
      const key = e.actor === null ? null : e.actor.kind === "claude-code" ? "claudeCode" : "other";
      map.set(e.decisionId, key === null ? t("grd.agent.none") : e.worktree === null ? t(`grd.agent.${key}`) : t(`grd.actor.${key}`, { worktree: e.worktree.folder }));
    }
    return map;
  }, [data, t]);

  useEffect(() => {
    heading.current?.focus();
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onBack();
  };

  const list = (group: BlockRow["group"]) => {
    const items = rows.filter((r) => r.group === group);
    if (items.length === 0) return null;
    return (
      <ul className="m-0 flex list-none flex-col gap-(--gr-space-2) p-0">
        {items.map((row) => (
          <li key={row.id}>
            {row.kind === "denial" ? (
              <button
                type="button"
                aria-label={row.a11yLabel}
                onClick={() => onOpenRow(row)}
                className="flex w-full items-start gap-(--gr-space-2) rounded-md bg-surface-control px-(--gr-space-3) py-(--gr-space-2) text-left text-text-main gr-ring gr-hover:bg-surface-hover"
              >
                <RowBody row={row} actor={actors.get(row.id) ?? ""} />
              </button>
            ) : (
              <div className="flex w-full items-start rounded-md px-(--gr-space-3) py-(--gr-space-2)">
                <RowBody row={row} actor={actors.get(row.id) ?? ""} />
              </div>
            )}
          </li>
        ))}
      </ul>
    );
  };

  let content;
  if (data === undefined) {
    content = <Skeleton />;
  } else {
    const unavailable = data.state === "unavailable";
    const protection = data.protection?.state ?? null;
    const unavailableText =
      data.state !== "unavailable"
        ? null
        : data.cause === "not-observed"
          ? t("grd.unavailable.notObserved", { repo: repoName })
          : protection === "hooks-only" || protection === "full"
            ? t("grd.unavailable.active", { repo: repoName })
            : t("grd.unavailable.unknown", { repo: repoName });
    const ready = data.state === "ready" ? data : null;
    const days = ready === null ? 0 : Math.max(1, Math.round((nowMs - ready.sinceMs) / DAY_MS));
    content = (
      <>
        {unavailableText !== null ? (
          <StatePanel title={t("grd.nav.title")} message={<p className="m-0">{unavailableText}</p>} className="mx-0 max-w-none" />
        ) : null}
        {data.state === "unavailable" && data.entries.length > 0 && data.readAtMs !== null ? (
          <p className={muted}>{t("grd.stale", { age: formatAge(Math.max(0, nowMs - data.readAtMs), t) })}</p>
        ) : null}
        {ready !== null ? (
          <p className={muted}>{t("grd.summary", { blocked: ready.summary.blocked, days, exceptions: ready.summary.exceptions })}</p>
        ) : null}
        {ready?.pending != null ? <PendingException pending={ready.pending} copied={copied} /> : null}
        {!unavailable && rows.length === 0 ? (
          <p className="m-0 text-text-main [font:var(--gr-type-body)]">
            {t(protection === "unprotected" ? "grd.empty.unprotected" : "grd.empty", { repo: repoName })}
          </p>
        ) : null}
        {list("blocks")}
        {rows.some((r) => r.group === "unverified") ? (
          <section className="flex flex-col gap-(--gr-space-2)">
            <h3 className="m-0 text-text-main [font:var(--gr-type-body-strong)]">{t("grd.unverified")}</h3>
            {list("unverified")}
          </section>
        ) : null}
        {ready?.unlogged.map((period) => (
          <p key={`${period.fromMs}-${period.toMs ?? "open"}`} className={muted}>
            {t("grd.unlogged", { from: formatMoment(period.fromMs, lang), to: formatMoment(period.toMs ?? nowMs, lang) })}
          </p>
        ))}
      </>
    );
  }

  return (
    <section onKeyDown={onKeyDown} className="flex min-h-0 flex-1 flex-col gap-(--gr-space-3) overflow-auto">
      <div>
        <Button variant="link" size="sm" onClick={onBack}>
          {t("grd.view.back")}
        </Button>
      </div>
      <h2 ref={heading} tabIndex={-1} className="m-0 text-text-main [font:var(--gr-type-title)]">
        {t("grd.nav.title")} · {repoName}
      </h2>
      {content}
    </section>
  );
}
