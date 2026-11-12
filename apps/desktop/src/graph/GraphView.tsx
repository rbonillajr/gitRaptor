import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useShortcuts } from "@/shell/useShortcuts";
import { useEngineStore } from "@/state/engine-store";
import { SegmentedControl, StatePanel, Table, Tooltip, type TableColumn } from "@/ui";
import { cx, type CssVars } from "@/ui/cx";
import { ColumnSettings } from "./ColumnSettings";
import { GraphCanvas } from "./GraphCanvas";
import { commitDateFormatter } from "./graph-model";
import { HEADER_PX, ROW_PX, graphLayout } from "./graph-layout";
import { COLUMN_PX, MESSAGE_MIN_PX, useGraphPrefs, visibleColumns } from "./graph-prefs";
import type { GraphColumnId, GraphLayer, GraphModel, GraphRow, LaneModel } from "./graph-types";
import { LaneTable } from "./LaneTable";

export interface GraphViewProps {
  model: GraphModel;
  repoName: string;
  stale: boolean;
  layers?: readonly GraphLayer[];
  /** A node for the Branch cell of a lane head and for the table view: the conflict badge (T011). */
  laneBadge?: (key: string) => ReactNode;
}

/** Rows painted beyond the visible ones on each side, never past the visible count itself (NFR-13.3). */
const OVERSCAN = 8;
/** Rows to assume until the container has been measured. */
const UNMEASURED_VISIBLE = 20;
const SHA_DIGITS = 7;
const NO_LAYERS: readonly GraphLayer[] = [];
const MAX_TIMING_ENTRIES = 1000;

/** Static class names, so Tailwind finds them: the lane colour of a slot (1 to 7). */
const LANE_DOT = ["bg-lane-1", "bg-lane-2", "bg-lane-3", "bg-lane-4", "bg-lane-5", "bg-lane-6", "bg-lane-7"] as const;
const dotClass = (slot: number | null) => (slot === null ? "bg-text-muted" : (LANE_DOT[Math.min(Math.max(slot, 1), LANE_DOT.length) - 1] ?? "bg-text-muted"));

const TRACK: Record<GraphColumnId, string> = {
  branch: `${COLUMN_PX.branch}px`,
  graph: `${COLUMN_PX.graph}px`,
  message: `minmax(${MESSAGE_MIN_PX}px, 1fr)`,
  author: `${COLUMN_PX.author}px`,
  date: `${COLUMN_PX.date}px`,
  sha: `${COLUMN_PX.sha}px`,
};

interface Size {
  width: number;
  height: number;
}

/**
 * The size of the element, read at once when it mounts and then from a `ResizeObserver` coalesced to one
 * change per frame: no loop while nothing resizes (NFR-13.8).
 */
function useElementSize(ref: { current: HTMLElement | null }): Size {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (el === null) return;
    const apply = (width: number, height: number) =>
      setSize((prev) => (prev.width === width && prev.height === height ? prev : { width, height }));
    apply(el.clientWidth, el.clientHeight);
    let frame: number | null = null;
    let latest: Size | null = null;
    const observer = new ResizeObserver((entries) => {
      const rect = entries.at(-1)?.contentRect;
      if (rect === undefined) return;
      latest = { width: Math.round(rect.width), height: Math.round(rect.height) };
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        if (latest !== null) apply(latest.width, latest.height);
      });
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, [ref]);
  return size;
}

/** The marks of NFR-13.2 and 13.4: the first paint of the graph, and each graph delta measured once. */
function useGraphMarks(ready: boolean): void {
  const batch = useEngineStore((s) => s.lastGraphBatch);
  const measured = useRef(useEngineStore.getState().lastGraphBatch);
  const firstPaint = useRef(false);

  useEffect(() => {
    if (!ready) return;
    const frame = requestAnimationFrame(() => performance.mark("gr:graph-painted"));
    return () => cancelAnimationFrame(frame);
  }, [ready]);

  useEffect(() => {
    if (batch === null || batch === measured.current) return;
    const frame = requestAnimationFrame(() => {
      measured.current = batch;
      performance.measure("gr:graph-delta", { start: batch.receivedAt });
      if (!firstPaint.current) {
        firstPaint.current = true;
        performance.mark("gr:graph-first-paint");
      }
      // The platform never clears the User Timing buffer: cap it.
      if (performance.getEntriesByName("gr:graph-delta").length > MAX_TIMING_ENTRIES) {
        performance.clearMeasures("gr:graph-delta");
        performance.clearMarks("gr:graph-first-paint");
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [batch]);
}

/** The base problem of D8 as the text the person reads; `null` when the lanes are drawn. */
function baseProblem(model: GraphModel, t: ReturnType<typeof useTranslation>["t"]): { title: string; hint: string | null } | null {
  const base = model.baseName ?? "";
  switch (model.state) {
    case "base-unconfirmed":
      return { title: t("graph.base.unconfirmed", { base }), hint: null };
    case "no-base":
      return { title: t("graph.base.none"), hint: t("graph.base.missingHint") };
    case "base-missing":
      return { title: t("graph.base.missing", { base }), hint: t("graph.base.missingHint") };
    case "repo-unavailable":
      return { title: t("graph.base.repoUnavailable"), hint: null };
    default:
      return null;
  }
}

/**
 * The lane graph (DS-US-DSK-005): a virtualised grid of the model's rows with the lane canvas over its Graph
 * column, Column settings and the table view as the textual alternative. Presentational: the model, the
 * conflict layer and the lane badge come in as props.
 */
export function GraphView({ model, repoName, stale, layers = NO_LAYERS, laneBadge }: GraphViewProps): ReactElement {
  const { t, i18n } = useTranslation();
  const savedColumns = useGraphPrefs((s) => s.columns);
  const view = useGraphPrefs((s) => s.view);
  const prefs = useMemo(() => ({ columns: savedColumns, view }), [savedColumns, view]);
  const setView = useGraphPrefs((s) => s.setView);
  const scroller = useRef<HTMLDivElement>(null);
  const size = useElementSize(scroller);
  const [top, setTop] = useState(0);
  const [selected, setSelected] = useState<{ id: string; laneKey: string } | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");

  // The stored preference is the truth on mount (the store is a module singleton that outlives the view).
  useEffect(() => {
    void useGraphPrefs.persist.rehydrate();
  }, []);

  const toggleView = () => {
    const next = prefs.view === "graph" ? "table" : "graph";
    setView(next);
    setAnnouncement(t(`graph.view.announce.${next}`));
  };
  useShortcuts({ onToggleGraphView: toggleView });

  const measured = size.width > 0;
  const columns = useMemo(() => visibleColumns(prefs, measured ? size.width : Infinity), [prefs, measured, size.width]);
  const geometry = useMemo(() => graphLayout(model, COLUMN_PX.graph), [model]);
  const locale = i18n.language;
  const formatDate = useMemo(() => commitDateFormatter(locale), [locale]);

  const total = model.rows.length;
  const visible = size.height > 0 ? Math.ceil(size.height / ROW_PX) : UNMEASURED_VISIBLE;
  const overscan = Math.min(OVERSCAN, Math.floor(visible / 2));
  const first = Math.max(0, Math.min(top, Math.max(total - 1, 0)) - overscan);
  const last = Math.min(total - 1, top + visible - 1 + overscan);
  const rows = model.rows.slice(first, Math.max(first, last + 1));
  const win = { first, last: Math.max(first, last) };

  useGraphMarks(model.state !== "loading");

  const lanes = useMemo(() => new Map(model.lanes.map((l) => [l.key, l])), [model.lanes]);
  const wipIds = useMemo(() => new Set(model.rows.filter((r) => r.kind === "wip").map((r) => r.id)), [model.rows]);

  // Selection survives a new model by row id; if the row left, it moves to the head of its lane.
  const selectedId = useMemo(() => {
    if (selected === null) return null;
    if (model.rows.some((r) => r.id === selected.id)) return selected.id;
    const head = model.rows.find((r) => (r.kind === "commit" && r.head && r.laneKey === selected.laneKey) || (r.kind === "lane" && r.laneKey === selected.laneKey));
    return head?.id ?? null;
  }, [selected, model.rows]);

  // Once the row the caller asked to focus is painted the table has focused it: forget the request.
  useEffect(() => {
    if (focusId !== null && rows.some((r) => r.id === focusId)) setFocusId(null);
  }, [focusId, rows]);

  const scrollTo = (index: number) => {
    const el = scroller.current;
    if (el !== null) {
      const above = index * ROW_PX;
      const below = (index + 1) * ROW_PX - Math.max(size.height - HEADER_PX, ROW_PX);
      if (above < el.scrollTop) el.scrollTop = above;
      else if (below > el.scrollTop) el.scrollTop = below;
      setTop(Math.floor(el.scrollTop / ROW_PX));
    }
    const row = model.rows[index];
    if (row !== undefined) setFocusId(row.id);
  };

  const gridColumns: TableColumn[] = columns.map((id) => ({
    id,
    header: t(`graph.column.${id}`),
    track: TRACK[id],
    ...(id === "graph" ? { paintHover: false } : {}),
    ...(id === "message" ? { primary: true } : {}),
  }));
  const leftOfGraph = columns.includes("branch") ? COLUMN_PX.branch : 0;
  const minWidth = columns.reduce((sum, id) => sum + (id === "message" ? MESSAGE_MIN_PX : COLUMN_PX[id]), 0);

  const lane = (row: GraphRow): LaneModel | undefined => (row.kind === "base" ? undefined : lanes.get(row.laneKey));
  const baseName = model.baseName ?? "";

  const renderCell = (row: GraphRow, id: string): ReactNode => {
    const l = lane(row);
    switch (id) {
      case "branch":
        if ((row.kind === "commit" && row.head) || row.kind === "lane") return l ? <BranchCell lane={l} badge={laneBadge?.(l.key)} /> : null;
        if (row.kind === "base") return row.name === "" ? null : <span className="min-w-0 truncate">{t("graph.base.label", { base: row.name })}</span>;
        return null;
      case "graph":
        return <span aria-hidden="true" />;
      case "message":
        return <MessageCell row={row} lane={l} baseName={baseName} showBranch={!columns.includes("branch")} t={t} />;
      case "author":
        return row.kind === "commit" ? (
          <Tooltip content={t("graph.author.tooltip")}>
            <span tabIndex={-1} className="text-text-muted [font:var(--gr-type-meta)]">
              {t("graph.author.unattributed")}
            </span>
          </Tooltip>
        ) : null;
      case "date":
        return row.kind === "commit" ? <span className="text-text-muted [font:var(--gr-type-meta)]">{formatDate(row.committedUtcMs)}</span> : null;
      case "sha":
        return row.kind === "commit" ? <code className="text-text-muted [font:var(--gr-type-mono)]">{row.oid.slice(0, SHA_DIGITS)}</code> : null;
      default:
        return null;
    }
  };

  const problem = baseProblem(model, t);
  const cutNote = t("graph.lane.cut");
  const cutShown = model.lanes.length - model.rows.filter((r) => r.kind === "lane" && r.note === cutNote).length;
  const notes: string[] = [];
  if (model.note === "unsupported") notes.push(t("graph.unsupported"));
  else if (model.note === "failed") notes.push(t("graph.failed"));
  else if (model.note === "cut") notes.push(t("graph.cut", { shown: cutShown, total: model.lanes.length }));
  if (prefs.view === "graph" && geometry.clipped > 0) notes.push(t("graph.clipped", { shown: model.lanes.length - geometry.clipped, total: model.lanes.length }));

  const pad: CssVars = {
    "--gr-graph-pad-top": `${first * ROW_PX}px`,
    "--gr-graph-pad-bottom": `${Math.max(0, total - 1 - win.last) * ROW_PX}px`,
    "--gr-graph-min-width": `${minWidth}px`,
  };

  return (
    <section data-graph={model.state === "loading" ? "loading" : "ready"} data-stale={stale ? "true" : undefined} className={cx("flex min-h-0 flex-1 flex-col gap-(--gr-space-2)", stale && "text-text-muted")}>
      <div className="flex items-center justify-end">
        <Tooltip content={t("graph.view.shortcut", { label: t("graph.view.label") })}>
          <span className="inline-flex">
            <SegmentedControl
              label={t("graph.view.label")}
              options={[
                { value: "graph", label: t("graph.view.graph") },
                { value: "table", label: t("graph.view.table") },
              ]}
              value={prefs.view}
              onChange={(next) => {
                if (next === prefs.view) return;
                setView(next);
                setAnnouncement(t(`graph.view.announce.${next}`));
              }}
            />
          </span>
        </Tooltip>
      </div>
      <div role="status" className={problem === null ? "sr-only" : undefined}>
        {problem !== null ? (
          <StatePanel title={problem.title} {...(problem.hint !== null ? { message: <p className="m-0">{problem.hint}</p> } : {})} />
        ) : null}
        {problem !== null && announcement !== "" ? <span className="sr-only">{announcement}</span> : null}
        {problem === null ? announcement : null}
      </div>
      {notes.map((note) => (
        <p key={note} className="m-0 text-text-muted [font:var(--gr-type-secondary)]">
          {note}
        </p>
      ))}
      {model.state === "ready" && model.empty ? <p className="m-0 text-text-muted [font:var(--gr-type-body)]">{t("graph.empty.noLanes")}</p> : null}
      <div
        ref={scroller}
        onScroll={(e) => setTop(Math.floor(e.currentTarget.scrollTop / ROW_PX))}
        className="min-h-0 flex-1 overflow-auto"
      >
        {prefs.view === "table" ? (
          <LaneTable model={model} repoName={repoName} stale={stale} {...(laneBadge ? { laneBadge } : {})} />
        ) : (
          <div style={pad} className="relative min-w-(--gr-graph-min-width)">
            <Table<GraphRow>
              label={t("graph.title", { repo: repoName })}
              columns={gridColumns}
              rows={rows}
              totalRows={total}
              firstRowIndex={first}
              getRowId={(r) => r.id}
              getRowLabel={(r) => r.a11yLabel}
              renderCell={renderCell}
              selectedId={selectedId}
              onSelect={(id) => {
                const row = model.rows.find((r) => r.id === id);
                if (row !== undefined) setSelected({ id, laneKey: row.kind === "base" ? "base" : row.laneKey });
              }}
              onActivate={() => undefined}
              wipIds={wipIds}
              focusId={focusId}
              onFocusOutside={scrollTo}
              loading={model.state === "loading"}
              disabled={stale}
              settings={<ColumnSettings availableWidthPx={measured ? size.width : Infinity} />}
              className="pt-(--gr-graph-pad-top) pb-(--gr-graph-pad-bottom)"
            />
            {columns.includes("graph") && model.state !== "loading" && total > 0 ? (
              <GraphCanvas model={model} geometry={geometry} window={win} layers={layers} leftPx={leftOfGraph} />
            ) : null}
          </div>
        )}
      </div>
    </section>
  );
}

function BranchCell({ lane, badge }: { lane: LaneModel; badge: ReactNode }) {
  return (
    <span className="flex min-w-0 items-center gap-(--gr-space-2)">
      <span aria-hidden="true" className="shrink-0">
        {lane.symbol}
      </span>
      <code className="min-w-0 truncate [font:var(--gr-type-mono)]">{lane.branchText}</code>
      <span aria-hidden="true" className={cx("size-(--gr-space-2) shrink-0 rounded-pill", dotClass(lane.muted ? null : lane.colorSlot))} />
      <span className={cx("min-w-0 truncate", lane.muted ? "text-text-muted" : "text-text-main")}>{lane.agentLabel}</span>
      {badge}
    </span>
  );
}

function MessageCell({
  row,
  lane,
  baseName,
  showBranch,
  t,
}: {
  row: GraphRow;
  lane: LaneModel | undefined;
  baseName: string;
  showBranch: boolean;
  t: ReturnType<typeof useTranslation>["t"];
}): ReactNode {
  switch (row.kind) {
    case "commit":
      return (
        <span className="min-w-0 truncate">
          {showBranch && row.head && lane ? <code className="[font:var(--gr-type-mono)]">{lane.branchText} </code> : null}
          {row.summary}
        </span>
      );
    case "wip":
      return t("graph.wip", { count: row.files });
    case "lane":
      return (
        <span className="flex min-w-0 items-center gap-(--gr-space-2) text-text-muted">
          {showBranch && lane ? <code className="[font:var(--gr-type-mono)]">{lane.branchText}</code> : null}
          <span className="truncate">{row.syncText}</span>
          {row.note !== "" ? <span className="truncate">{row.note}</span> : null}
        </span>
      );
    case "more":
      return <span className="text-text-muted">{row.exact ? t("graph.more", { count: row.count }) : t("graph.moreAtLeast", { count: row.count })}</span>;
    case "fork":
      return <span className="text-text-muted">{t("graph.fork", { base: baseName, commit: row.mergeBase.slice(0, SHA_DIGITS) })}</span>;
    default:
      return null;
  }
}
