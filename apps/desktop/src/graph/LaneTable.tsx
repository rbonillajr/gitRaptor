import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Table, type TableColumn } from "@/ui";
import { tidy } from "./graph-model";
import type { GraphModel, LaneModel } from "./graph-types";

export interface LaneTableProps {
  model: GraphModel;
  repoName: string;
  stale: boolean;
  laneBadge?: (key: string) => ReactNode;
}

const COLUMNS = ["branch", "agent", "state", "sync", "commits"] as const;
/** Flexible tracks: the table has no fixed widths, it reads as a list. */
const TRACKS: Record<(typeof COLUMNS)[number], string> = {
  branch: "minmax(0, 1.2fr)",
  agent: "minmax(0, 1.4fr)",
  state: "minmax(0, 0.8fr)",
  sync: "minmax(0, 1.4fr)",
  commits: "minmax(0, 0.8fr)",
};

/**
 * The textual alternative of the graph (D14): one row per lane with branch, agent, state, ahead/behind and
 * commits (shown of total, or "—" while the commits have not arrived). Every lane is listed, drawn or not.
 */
export function LaneTable({ model, repoName, stale, laneBadge }: LaneTableProps) {
  const { t } = useTranslation();
  const columns: TableColumn[] = COLUMNS.map((id) => ({ id, header: t(`graph.table.${id}`), track: TRACKS[id] }));
  const commitsText = (lane: LaneModel) => (lane.total === null ? "—" : t("graph.table.commitsOf", { shown: lane.shown, total: `${lane.total.count}${lane.total.exact ? "" : "+"}` }));

  const cell = (lane: LaneModel, id: string): ReactNode => {
    switch (id) {
      case "branch":
        return (
          <span className="flex min-w-0 items-center gap-(--gr-space-2)">
            <code className="min-w-0 truncate [font:var(--gr-type-mono)]">{lane.branchText}</code>
            {laneBadge?.(lane.key)}
          </span>
        );
      case "agent":
        return <span className={lane.muted ? "text-text-muted" : "text-text-main"}>{lane.agentLabel}</span>;
      case "state":
        return (
          <span>
            <span aria-hidden="true">{lane.symbol} </span>
            {lane.stateText}
          </span>
        );
      case "sync":
        return lane.syncText;
      default:
        return commitsText(lane);
    }
  };

  return (
    <Table<LaneModel>
      label={t("graph.table.label", { repo: repoName })}
      columns={columns}
      rows={model.lanes}
      getRowId={(lane) => lane.key}
      getRowLabel={(lane) =>
        [tidy(t("graph.a11y.lane", { branch: lane.branchText, agent: lane.agentLabel, state: lane.stateText, sync: lane.syncText })), commitsText(lane)].join(", ")
      }
      renderCell={cell}
      selectedId={null}
      onSelect={() => undefined}
      onActivate={() => undefined}
      loading={model.state === "loading"}
      disabled={stale}
    />
  );
}
