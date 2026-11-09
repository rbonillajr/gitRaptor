import type { ReactNode } from "react";
import { cx } from "./cx";
import { Panel } from "./Panel";

export interface StatePanelProps {
  title: string;
  /** Plain text or nodes (a `<code>`, a `CopyCommand`); untrusted strings go in as React text only. */
  message?: ReactNode;
  /** The one thing the user can do about it. */
  action?: ReactNode;
  /** `error` tints the title with the danger text colour. */
  tone?: "neutral" | "error";
  className?: string;
}

/** A centred panel that explains a state: title, message and at most one action. */
export function StatePanel({ title, message, action, tone = "neutral", className }: StatePanelProps) {
  return (
    <Panel
      data-tone={tone}
      className={cx("mx-auto w-full max-w-(--gr-size-dialog-width) items-start gap-(--gr-space-3)", className)}
    >
      <h2 className={cx("m-0 [font:var(--gr-type-body-strong)]", tone === "error" ? "text-danger-text" : "text-text-main")}>{title}</h2>
      {message ? <div className="text-text-muted [font:var(--gr-type-body)]">{message}</div> : null}
      {action ? <div className="flex flex-wrap items-center gap-(--gr-space-2)">{action}</div> : null}
    </Panel>
  );
}
