import { skipToken, useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { engineKeys } from "@/client/queries";
import type { GuardBlocks } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { Button, CopyCommand, Dialog } from "@/ui";
import { blockRows, operationKey, REBASE_FORM, type BlockRow, type CopiedException } from "./blocks-model";
import { PendingException } from "./PendingException";

export interface BlockDialogProps {
  row: BlockRow;
  repoId: string;
  fromToast?: boolean;
  onClose: () => void;
  onCopied: (copied: NonNullable<CopiedException>) => void;
}

const h3 = "m-0 text-text-main [font:var(--gr-type-body-strong)]";
const body = "m-0 text-text-main [font:var(--gr-type-body)]";
const muted = "m-0 text-text-muted [font:var(--gr-type-body)]";
const code = "break-all rounded-md bg-surface-control px-(--gr-space-2) py-(--gr-space-1) text-text-main [font:var(--gr-type-mono)]";

/**
 * The block explained in two steps (D14, D17). Step 1 (d1): who tried what, why it is dangerous and which rule stopped
 * it. Step 2 (d4): the exact command to run in the person's own terminal, with Copy: the app never runs it. Closing
 * (Esc, outside click, the X, "Close") sends nothing to the engine. The announced exception and its outcome come from
 * the cache, which the engine fills when the terminal acts.
 */
export function BlockDialog({ row, repoId, fromToast = false, onClose, onCopied }: BlockDialogProps) {
  const { t, i18n } = useTranslation();
  const [step, setStep] = useState<1 | 2>(1);
  const [copiedHere, setCopiedHere] = useState<CopiedException>(null);
  const anchor = useRef<HTMLSpanElement>(null);
  const data = useQuery<GuardBlocks>({ queryKey: engineKeys.guard(repoId), queryFn: skipToken }).data;
  const entry = data?.entries.find((e) => e.decisionId === row.id);

  // The "I'll do it myself…" button disappears with the step: the focus goes to the footer button, not to body.
  useEffect(() => {
    if (step === 2) anchor.current?.closest('[role="dialog"]')?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }, [step]);

  const offered = row.exception !== null && ("command" in row.exception || "rebaseForm" in row.exception);
  const notOffered = row.exception !== null && "notOffered" in row.exception ? row.exception.notOffered : null;
  const engineDown = row.ctaDisabled === "engine-down";
  const agent = entry?.actor == null ? t("grd.agent.none") : t(entry.actor.kind === "claude-code" ? "grd.agent.claudeCode" : "grd.agent.other");

  const outcome =
    entry === undefined || data === undefined
      ? undefined
      : data.entries
          .filter((e) => e.kind !== "denial" && e.lastMs >= entry.atMs && operationKey(e.operation) === operationKey(entry.operation))
          .sort((a, b) => b.lastMs - a.lastMs)[0];
  const outcomeTitle =
    outcome === undefined || data === undefined ? undefined : blockRows(data, true, t, i18n.language).find((r) => r.id === outcome.decisionId)?.title;
  const pending = data?.state === "ready" ? data.pending : null;
  const showPending =
    pending !== null && entry !== undefined && (operationKey(pending.operation) === operationKey(entry.operation) || copiedHere !== null);

  const copy = () => {
    if (entry === undefined) return;
    const copied = { repoId, operation: entry.operation, rules: entry.rules.map((r) => r.rule) };
    setCopiedHere(copied);
    onCopied(copied);
  };

  const seeLog = () => {
    useUiStore.getState().openGuardrails();
    onClose();
  };

  const firstStepBody = (
    <>
      <span ref={anchor} />
      {row.nothingHappened !== null ? (
        <section className="flex flex-col gap-(--gr-space-1)">
          <h3 className={h3}>{t("grd.block.attempted")}</h3>
          <p className={body}>{row.attempted}</p>
        </section>
      ) : null}
      {row.why !== null ? (
        <section className="flex flex-col gap-(--gr-space-1)">
          <h3 className={h3}>{t("grd.block.whyLabel")}</h3>
          <p className={body}>{row.why}</p>
        </section>
      ) : null}
      <section className="flex flex-col gap-(--gr-space-1)">
        <h3 className={h3}>{t("grd.block.stoppedLabel")}</h3>
        <ul className="m-0 flex list-none flex-col gap-(--gr-space-1) p-0">
          {row.stoppedBy.map((line, i) => (
            <li key={`${i}-${line}`} className={body}>
              {line}
            </li>
          ))}
        </ul>
      </section>
      <details className="text-text-muted [font:var(--gr-type-secondary)]">
        <summary className="cursor-pointer gr-ring">{t("grd.block.technical")}</summary>
        <ul className="m-0 flex list-none flex-col gap-(--gr-space-1) p-0 pt-(--gr-space-2)">
          {row.technical.map((line, i) => (
            <li key={`${i}-${line}`} className="[overflow-wrap:anywhere]">
              {line}
            </li>
          ))}
        </ul>
      </details>
      <section className="flex flex-col gap-(--gr-space-1)">
        <h3 className={h3}>{t("grd.block.canDo")}</h3>
        <p className={body}>{t(offered ? "grd.block.do.exception" : "grd.block.do.nothing")}</p>
        {notOffered !== null ? <p className={muted}>{notOffered}</p> : null}
        {engineDown && offered ? <p className={muted}>{t("grd.exc.engineDown")}</p> : null}
      </section>
      <div className="flex flex-wrap justify-end gap-(--gr-space-2)">
        {fromToast ? (
          <Button variant="link" onClick={seeLog}>
            {t("grd.block.seeLog")}
          </Button>
        ) : null}
        {offered ? (
          <Button
            variant="secondary"
            disabled={engineDown}
            onClick={() => {
              if (!engineDown) setStep(2);
            }}
          >
            {t("grd.exc.cta")}
          </Button>
        ) : null}
      </div>
    </>
  );

  const secondStepBody = (
    <>
      <span ref={anchor} />
      <p className={body}>{t("grd.exc.who", { agent })}</p>
      {outcome !== undefined && outcomeTitle !== undefined ? (
        <p role="status" className={body}>
          {outcomeTitle}
        </p>
      ) : (
        <>
          {row.exception !== null && "rebaseForm" in row.exception ? null : row.exception !== null && "command" in row.exception ? (
            <>
              {entry?.operation.kind === "push" ? <p className={body}>{t("grd.exc.remote")}</p> : null}
              <p className={body}>{t("grd.exc.terminal")}</p>
              <CopyCommand command={row.exception.command} onCopied={() => copy()} />
              <p className={muted}>{t("grd.exc.where")}</p>
              <p className={muted}>{t("grd.exc.review")}</p>
            </>
          ) : null}
          {row.exception !== null && "rebaseForm" in row.exception ? (
            <>
              <p className={body}>{t("grd.exc.terminal")}</p>
              <code className={code}>{REBASE_FORM}</code>
              <p className={muted}>{t("grd.exc.rebaseHint")}</p>
            </>
          ) : null}
          {row.hint !== null ? <p className={muted}>{row.hint}</p> : null}
          {showPending && pending !== null ? <PendingException pending={pending} copied={copiedHere} /> : <p className={muted}>{t("grd.exc.cancelHow")}</p>}
        </>
      )}
    </>
  );

  return (
    <Dialog
      open
      kind="info"
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={
        step === 1 ? (
          <>
            <span aria-hidden="true">⛔ </span>
            {row.title}
          </>
        ) : (
          t("grd.exc.terminal.title")
        )
      }
      description={step === 1 ? (row.nothingHappened ?? row.attempted) : t("grd.exc.reason")}
      acknowledge={{ label: step === 1 ? t("grd.block.ok") : t("grd.exc.close") }}
    >
      {step === 1 ? firstStepBody : secondStepBody}
    </Dialog>
  );
}
