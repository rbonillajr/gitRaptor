import { useTranslation } from "react-i18next";
import type { LimitCode } from "@/client/engine-types";
import { Button, Popover } from "@/ui";

const LIMIT_KEY = {
  "overlap-includes-uncommitted": "conflicts.limit.overlapIncludesUncommitted",
  "conflict-committed-only": "conflicts.limit.conflictCommittedOnly",
  "no-merge-drivers-or-attributes": "conflicts.limit.noMergeDriversOrAttributes",
  "rename-limit": "conflicts.limit.renameLimit",
  "insertions-may-merge": "conflicts.limit.insertionsMayMerge",
  "joined-hunks": "conflicts.limit.joinedHunks",
  "pair-ignores-base-moves": "conflicts.limit.pairIgnoresBaseMoves",
  "local-only": "conflicts.limit.localOnly",
} as const satisfies Record<LimitCode, string>;

/** The one-line limits note and its "Limits (?)" popover (D5). `onOpenChange` lets a view know a layer is open (D18). */
export function LimitsLine({ limits, onOpenChange }: { limits: readonly LimitCode[]; onOpenChange?: (open: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <p className="m-0 flex flex-wrap items-center gap-(--gr-space-2) text-text-muted [font:var(--gr-type-secondary)]">
      {t("conflicts.limits.line")}
      <Popover
        label={t("conflicts.limits.title")}
        {...(onOpenChange ? { onOpenChange } : {})}
        trigger={(props) => (
          <Button variant="link" size="sm" {...props}>
            {t("conflicts.limits.open")}
          </Button>
        )}
      >
        <ul className="m-0 flex list-disc flex-col gap-(--gr-space-1) pl-(--gr-space-4)">
          {limits.map((code) => (
            <li key={code}>{t(LIMIT_KEY[code])}</li>
          ))}
        </ul>
      </Popover>
    </p>
  );
}
