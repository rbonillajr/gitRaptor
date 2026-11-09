import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RepoSummaryDto } from "@/client/engine-types";
import { RepoBadge, StatusDot, Tree, type TreeNode } from "@/ui";
import type { CardModel } from "./fleet-model";

export interface RepoTreeProps {
  /** `undefined` while the global snapshot is not in. */
  repos: readonly RepoSummaryDto[] | undefined;
  selectedRepoId: string | null;
  /** The cards of the open repo, in fleet order: they become its "Worktrees" group. */
  cards: readonly CardModel[];
  /** A repo row was chosen (click, Enter or Space). Not called for the repo that is already open. */
  onOpen: (repoId: string) => void;
}

const repoNodeId = (repoId: string) => `repo:${repoId}`;
const groupNodeId = (repoId: string) => `group:${repoId}`;

/**
 * The sidebar repo tree: one row per repo and, under the open one, the "Worktrees" group in fleet order.
 * Presentational: the repo list, the selection and the open callback are props.
 */
export function RepoTree({ repos, selectedRepoId, cards, onOpen }: RepoTreeProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [seenRepo, setSeenRepo] = useState<string | null>(null);
  if (seenRepo !== selectedRepoId) {
    setSeenRepo(selectedRepoId);
    if (selectedRepoId !== null) setExpanded(new Set([repoNodeId(selectedRepoId), groupNodeId(selectedRepoId)]));
  }

  const nodes = useMemo<TreeNode[]>(
    () =>
      (repos ?? []).map((repo): TreeNode => {
        const unavailable = repo.state === "unavailable";
        const conflicts = repo.attention.conflicts.state === "counted" ? repo.attention.conflicts.count : 0;
        const conflictsLabel = conflicts > 0 ? t("repos.conflicts", { count: conflicts }) : null;
        const node: TreeNode = {
          id: repoNodeId(repo.repoId),
          kind: "repo",
          label: repo.name,
          accessibleName: [repo.name, unavailable ? t("repos.unavailable") : null, conflictsLabel].filter((p) => p !== null).join(", "),
          leading: <RepoBadge repoId={repo.repoId} name={repo.name} />,
          disabled: unavailable,
        };
        if (unavailable) node.meta = <span>{t("repos.unavailable")}</span>;
        else if (conflictsLabel !== null) {
          node.meta = (
            <span role="img" aria-label={conflictsLabel} className="text-danger-text">
              ⚡{conflicts}
            </span>
          );
        }
        if (repo.repoId === selectedRepoId && !unavailable && cards.length > 0) {
          node.children = [
            {
              id: groupNodeId(repo.repoId),
              kind: "group",
              label: t("repos.worktrees"),
              children: cards.map(
                (c): TreeNode => ({
                  id: `worktree:${repo.repoId}:${c.key}`,
                  kind: "item",
                  label: c.folder,
                  accessibleName: c.agentState === "active" || c.agentState === "inactive" ? `${c.folder}, ${c.stateText}` : c.folder,
                  ...(c.agentState === "active" || c.agentState === "inactive"
                    ? { leading: <StatusDot tone={c.agentState} decorative /> }
                    : {}),
                }),
              ),
            },
          ];
        }
        return node;
      }),
    [repos, selectedRepoId, cards, t],
  );

  const open = (id: string) => {
    const repo = repos?.find((r) => repoNodeId(r.repoId) === id);
    if (repo && repo.state !== "unavailable" && repo.repoId !== selectedRepoId) onOpen(repo.repoId);
  };

  return (
    <div className="flex min-h-0 flex-col">
      <h2 className="m-0 px-(--gr-space-3) py-(--gr-space-2) text-text-muted [font:var(--gr-type-caption)]">{t("repos.title")}</h2>
      <Tree
        label={t("repos.title")}
        nodes={nodes}
        expanded={expanded}
        onExpandedChange={setExpanded}
        selectedId={selectedRepoId === null ? null : repoNodeId(selectedRepoId)}
        onSelect={open}
        onActivate={() => {}}
        loading={repos === undefined}
      />
    </div>
  );
}
