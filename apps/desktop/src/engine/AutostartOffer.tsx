import { useTranslation } from "react-i18next";
import { Button } from "@/ui";

export interface AutostartOfferProps {
  offer: "offered" | "registered";
  /** The choice is in flight: the button stays `aria-disabled` and does not call `onChange`. */
  pending?: boolean;
  /** `true` asks to enable the autostart, `false` to stop it. The confirmation dialog is the backend's. */
  onChange: (enable: boolean) => void;
  /** Translated reason the last choice failed, announced next to the action. */
  error?: string | null;
}

/** The one row that offers (or stops) the autostart under the engine band. Pure: props in, callbacks out. */
export function AutostartOffer({ offer, pending = false, onChange, error = null }: AutostartOfferProps) {
  const { t } = useTranslation();
  const enable = offer === "offered";
  return (
    <div className="flex flex-wrap items-center gap-(--gr-space-2) border-b border-border-subtle bg-surface-panel px-(--gr-space-4) py-(--gr-space-2) text-text-main [font:var(--gr-type-secondary)]">
      <span>{t("engine.autostart.label")}</span>
      <Button size="sm" pending={pending} onClick={() => onChange(enable)}>
        {enable ? t("engine.autostart.enable") : t("engine.autostart.stop")}
      </Button>
      <span role="status" className="text-danger-text">
        {error}
      </span>
    </div>
  );
}
