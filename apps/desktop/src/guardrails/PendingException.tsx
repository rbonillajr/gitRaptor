import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { PendingExceptionDto } from "@/client/engine-types";
import { useNow } from "@/fleet/useNow";
import { CopyCommand } from "@/ui";
import { opPhrase, pendingMismatch, ruleText, type CopiedException } from "./blocks-model";

export interface PendingExceptionProps {
  pending: PendingExceptionDto;
  copied: CopiedException;
}

const SENTINEL = "\u0001";
/** The countdown is announced when it appears and at 5 s, never every second (D16, WCAG 4.1.3). */
const ANNOUNCE_AT_S = 5;

/**
 * The announced exception (D12, D16): a countdown figure per second (`aria-hidden`), a separate live region that
 * speaks at the start, at 5 s and at 0 s, how to cancel it in the terminal, and an alert when it is not the one the
 * person copied (M-01). There is no bar and no animation: the number is the only countdown, so reduced motion
 * changes nothing. The app never cancels it.
 */
export function PendingException({ pending, copied }: PendingExceptionProps) {
  const { t } = useTranslation();
  const nowMs = useNow();
  const remaining = Math.max(0, Math.ceil((pending.appliesAtMs - nowMs) / 1000));
  const [first] = useState(remaining);
  const mismatch = pendingMismatch(pending, copied);

  const announced = remaining <= 0 ? null : remaining > ANNOUNCE_AT_S ? first : Math.min(first, ANNOUNCE_AT_S);
  const [before = "", after = ""] = t("grd.exc.pending", { n: SENTINEL }).split(SENTINEL);
  const operation = opPhrase(pending.operation, null, t);
  const rules = pending.rules.map((r) => ruleText(r, t)).join(", ");

  return (
    <section className="flex flex-col gap-(--gr-space-2) rounded-md bg-warning-bg p-(--gr-space-3) text-text-main [font:var(--gr-type-body)]">
      {mismatch ? (
        <p role="alert" className="m-0 text-danger-text [font:var(--gr-type-body-strong)]">
          {t("grd.exc.mismatch")}
        </p>
      ) : null}
      {remaining > 0 ? (
        <p aria-hidden="true" className="m-0 [font:var(--gr-type-body-strong)]">
          {before}
          <span aria-hidden="true">{remaining}</span>
          {after}
        </p>
      ) : (
        <p aria-hidden="true" className="m-0 [font:var(--gr-type-body-strong)]">
          {t("grd.exc.pendingNow")}
        </p>
      )}
      <span role="status" className="sr-only">
        {announced === null ? t("grd.exc.pendingNow") : t("grd.exc.pendingAnnounce", { n: announced })}
      </span>
      <p className="m-0">{t("grd.exc.pendingOp", { operation, rules })}</p>
      <p className="m-0 text-text-muted">{t("grd.exc.cancelHow")}</p>
      {pending.cancelCommand !== null ? (
        <>
          <p className="m-0 text-text-muted">{t("grd.exc.cancelOr")}</p>
          <CopyCommand command={pending.cancelCommand} />
        </>
      ) : null}
    </section>
  );
}
