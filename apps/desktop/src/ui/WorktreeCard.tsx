import { useTranslation } from "react-i18next";
import { cx } from "./cx";

const LANE: Record<number, string> = {
  1: "text-lane-1",
  2: "text-lane-2",
  3: "text-lane-3",
  4: "text-lane-4",
  5: "text-lane-5",
  6: "text-lane-6",
  7: "text-lane-7",
};

export interface WorktreeCardProps {
  /** `●`, `◐`, `○`, `⚠` or `""` (no glyph); decorative, the state text says the same. */
  symbol: string;
  agentLabel: string;
  /** `glass.lane.<n>` for a live agent; `null` leaves the name muted or main. */
  colorSlot: number | null;
  muted: boolean;
  shared: boolean;
  stateText: string;
  main: boolean;
  /** `null` while the worktree is unavailable. */
  branchText: string | null;
  /** When `branchText` starts with this hash, it is set in `<code>`. */
  branchCommit: string | null;
  filesText: string | null;
  syncText: string | null;
  activityText: string | null;
  attentionText: string | null;
  unavailableText: string | null;
  /** The phrase of the visible parts joined with ", " (DSF-DSK-001 § 7). */
  a11yLabel: string;
  className?: string;
}

/**
 * One worktree of the fleet: agent and state, branch, metrics and, when it applies, the reason it asks for
 * attention. Pure presentation: it knows no engine type, only the texts already decided by the view model.
 */
export function WorktreeCard(p: WorktreeCardProps) {
  const { t } = useTranslation();
  const nameColor = p.muted || p.colorSlot === null ? "text-text-muted" : (LANE[p.colorSlot] ?? "text-text-muted");
  const commit = p.branchCommit !== null && p.branchText?.startsWith(p.branchCommit) ? p.branchCommit : null;

  return (
    <article
      aria-label={p.a11yLabel}
      data-attention={p.attentionText !== null ? "true" : undefined}
      className={cx(
        "flex flex-col gap-(--gr-space-1) rounded-md bg-surface-control p-(--gr-space-3) text-text-main [font:var(--gr-type-body)]",
        p.attentionText !== null && "border border-danger",
        p.className,
      )}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-(--gr-space-2)">
        {p.symbol !== "" ? (
          <span aria-hidden="true" className={nameColor}>
            {p.symbol}
          </span>
        ) : null}
        <span className={cx("min-w-0 truncate [font:var(--gr-type-body-strong)]", nameColor)}>{p.agentLabel}</span>
        {p.shared ? <span className="text-text-muted [font:var(--gr-type-caption)]">{t("fleet.agent.shared")}</span> : null}
        {p.stateText !== "" ? <span className="text-text-muted [font:var(--gr-type-caption)]">{p.stateText}</span> : null}
        {p.main ? (
          <span className="rounded-pill bg-surface-hover px-(--gr-space-2) text-text-muted [font:var(--gr-type-caption)]">{t("fleet.main")}</span>
        ) : null}
      </div>
      {p.unavailableText !== null ? <div className="text-warning-text">{p.unavailableText}</div> : null}
      {p.branchText !== null ? (
        <div className="min-w-0 truncate text-text-muted">
          {commit !== null ? (
            <>
              <code className="[font:var(--gr-type-mono)]">{commit}</code>
              {p.branchText.slice(commit.length)}
            </>
          ) : (
            p.branchText
          )}
        </div>
      ) : null}
      {p.filesText !== null || p.syncText !== null || p.activityText !== null ? (
        <div className="flex flex-wrap gap-x-(--gr-space-3) text-text-muted [font:var(--gr-type-caption)]">
          {[p.filesText, p.syncText, p.activityText].map((text, i) => (text === null ? null : <span key={i}>{text}</span>))}
        </div>
      ) : null}
      {p.attentionText !== null ? <div className="text-danger-text">{p.attentionText}</div> : null}
    </article>
  );
}
