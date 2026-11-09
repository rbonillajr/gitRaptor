import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";
import { cx } from "./cx";
import { Check } from "./icons";

export interface CopyCommandProps {
  /** Shown as `<code>` text and copied as is. The caller guarantees it is safe to paste into a shell. */
  command: string;
  className?: string;
}

/** A command in a `<code>` box with a Copy button (`navigator.clipboard.writeText`, no plugin). */
export function CopyCommand({ command, className }: CopyCommandProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const copy = () => {
    navigator.clipboard.writeText(command).then(
      () => setCopied(true),
      () => setCopied(false),
    );
  };

  return (
    <span className={cx("inline-flex max-w-full flex-wrap items-center gap-(--gr-space-2)", className)}>
      <code className="min-w-0 break-all rounded-md bg-surface-control px-(--gr-space-2) py-(--gr-space-1) text-text-main [font:var(--gr-type-mono)]">
        {command}
      </code>
      <Button size="sm" variant="secondary" icon={copied ? <Check size="sm" /> : undefined} onClick={copy}>
        {t("engine.action.copy")}
      </Button>
    </span>
  );
}
