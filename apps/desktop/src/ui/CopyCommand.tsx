import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";
import { cx } from "./cx";
import { Check } from "./icons";

export interface CopyCommandProps {
  /** Shown as `<code>` text and copied as is. The caller guarantees it is safe to paste into a shell. */
  command: string;
  className?: string;
  /** Called with the command only after a successful `writeText` (DS-US-DSK-008 D22). */
  onCopied?: (command: string) => void;
}

const FEEDBACK_MS = 2000;

/**
 * A command in a `<code>` box with a Copy button (`navigator.clipboard.writeText`, no plugin). The result is said in
 * text on the button for 2 s and once in a `role="status"` region for screen readers (WCAG 4.1.3, D22).
 */
export function CopyCommand({ command, className, onCopied }: CopyCommandProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const latest = useRef(onCopied);
  useEffect(() => {
    latest.current = onCopied;
  });

  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = () => {
    navigator.clipboard.writeText(command).then(
      () => {
        clearTimeout(timer.current);
        setCopied(true);
        setAnnouncement(t("ui.common.copied"));
        timer.current = setTimeout(() => {
          setCopied(false);
          setAnnouncement("");
        }, FEEDBACK_MS);
        latest.current?.(command);
      },
      () => {
        clearTimeout(timer.current);
        setCopied(false);
        setAnnouncement(t("ui.common.copyFailed"));
      },
    );
  };

  return (
    <span className={cx("inline-flex max-w-full flex-wrap items-center gap-(--gr-space-2)", className)}>
      <code className="min-w-0 break-all rounded-md bg-surface-control px-(--gr-space-2) py-(--gr-space-1) text-text-main [font:var(--gr-type-mono)]">
        {command}
      </code>
      <Button size="sm" variant="secondary" icon={copied ? <Check size="sm" /> : undefined} onClick={copy}>
        {copied ? t("ui.common.copied") : t("engine.action.copy")}
      </Button>
      <span role="status" className="sr-only">
        {announcement}
      </span>
    </span>
  );
}
