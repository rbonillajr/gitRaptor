import { useTranslation } from "react-i18next";
import { cx } from "@/ui/cx";

export interface GuardrailsNavRowProps {
  unseen: number;
  active: boolean;
  onOpen: () => void;
}

/** The `id` of the row: the Guardrails view gives the focus back to it on Esc (D18). */
export const GUARDRAILS_NAV_ID = "gr-guardrails-nav";

/** The sidebar footer row: "Guardrails" with the block symbol and, when there are unseen blocks, their count (D11, D18). */
export function GuardrailsNavRow({ unseen, active, onOpen }: GuardrailsNavRowProps) {
  const { t } = useTranslation();
  return (
    <button
      id={GUARDRAILS_NAV_ID}
      type="button"
      aria-current={active ? "page" : undefined}
      aria-label={unseen > 0 ? t("grd.nav.labelUnseen", { unseen }) : undefined}
      onClick={onOpen}
      className={cx(
        "flex min-h-(--gr-size-target-action) w-full items-center gap-(--gr-space-2) rounded-md px-(--gr-space-3) text-left text-text-main gr-ring gr-hover:bg-surface-hover [font:var(--gr-type-body)]",
        active && "bg-surface-hover",
      )}
    >
      <span aria-hidden="true">⛔</span>
      <span className="min-w-0 flex-1">{t("grd.nav.title")}</span>
      {unseen > 0 ? (
        <span
          role="img"
          aria-label={t("grd.nav.unseen", { count: unseen })}
          className="rounded-pill bg-danger-bg px-(--gr-space-2) text-danger-text [font:var(--gr-type-caption)]"
        >
          <span aria-hidden="true">{unseen}</span>
        </span>
      ) : null}
    </button>
  );
}
