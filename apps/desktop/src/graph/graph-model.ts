import type { TFunction } from "i18next";
import type { GraphDto, LaneDto, LaneGapReason, RepoFleet, WorktreeDto } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";
import type { GraphEntry, GraphModel, GraphRow, LaneModel } from "./graph-types";

interface GraphModelInput {
  fleet: RepoFleet | undefined;
  cards: readonly CardModel[];
  entry: GraphEntry | null;
  t: TFunction;
  locale: string;
  laneA11y?: (key: string) => string | null;
}

const SHA_DIGITS = 7;

type OwnLane = Extract<LaneDto["state"], { state: "ready" }>;

const GAP_KEY = {
  "worktree-unavailable": "graph.lane.gap.worktreeUnavailable",
  "no-commits": "graph.lane.gap.noCommits",
  "no-merge-base": "graph.lane.gap.noMergeBase",
  "too-slow": "graph.lane.gap.tooSlow",
  unreadable: "graph.lane.gap.unreadable",
} as const satisfies Record<LaneGapReason, string>;

const cmp = (a: string, b: string) => a.localeCompare(b);

/** The formatter of the Date / Time column and of the accessible names: one rule for both. */
export function commitDateFormatter(locale: string): (ms: number) => string {
  const format = new Intl.DateTimeFormat(locale, { dateStyle: "short", timeStyle: "short" });
  return (ms) => format.format(ms);
}

/** Collapses the empty parts a template leaves behind ("a, , b", a leading or a trailing comma). */
export const tidy = (sentence: string) => sentence.replace(/\s*,(\s*,)+/g, ",").replace(/^[,\s]+|[,\s]+$/g, "");

const filesOf = (w: WorktreeDto) => (w.status.state === "ready" ? w.status.counts.staged + w.status.counts.unstaged + w.status.counts.untracked : 0);

type State = GraphModel["state"];

/** D8: the base of the repo decides first; then what the worktrees say; then what `graph.lanes` says. */
function baseState(fleet: RepoFleet, graph: GraphDto | null): { state: State; name: string | null } {
  const { base, worktrees } = fleet.repo;
  if (base.status === "unconfirmed") return { state: "base-unconfirmed", name: base.name };
  if (base.status === "invalid") return { state: "no-base", name: base.name };
  if (worktrees.some((w) => w.status.state === "ready" && w.status.divergence.state === "base-missing")) {
    return { state: "base-missing", name: base.name };
  }
  if (graph !== null && graph.base.state === "not-computable") {
    return { state: graph.base.reason, name: graph.base.name ?? base.name };
  }
  return { state: "ready", name: base.name };
}

/**
 * The rows and lanes of the lane graph. Pure: it does not know whether the commits arrived (`entry` without a
 * graph gives summary rows) and does not touch the text of the repo, which React paints as text.
 *
 * @param laneA11y Extra words for the accessible name of every row of a lane (the conflict band, T011).
 */
export function graphModel({ fleet, cards, entry, t, locale, laneA11y }: GraphModelInput): GraphModel {
  if (fleet === undefined) return { state: "loading", baseName: null, lanes: [], rows: [], empty: false, note: null };

  const graph = entry?.graph ?? null;
  const { state, name: baseName } = baseState(fleet, graph);
  const worktrees = fleet.repo.worktrees;
  const cardOf = new Map(cards.map((c) => [c.key, c]));
  const formatDate = commitDateFormatter(locale);
  const named = (key: string, sentence: string) => {
    const more = key === "base" ? null : (laneA11y?.(key) ?? null);
    return more === null || more === "" ? tidy(sentence) : `${tidy(sentence)}, ${more}`;
  };

  const trunk =
    baseName === null ? undefined : worktrees.find((w) => w.status.state === "ready" && w.status.head.kind === "branch" && w.status.head.name === baseName);
  // D5: stable columns by folder, path and key; never the attention order of the fleet.
  const laneWorktrees = worktrees
    .filter((w) => w !== trunk)
    .sort((a, b) => cmp(a.folder, b.folder) || cmp(a.path, b.path) || cmp(a.key, b.key));
  const graphLanes = new Map<string, LaneDto>((graph?.lanes ?? []).map((l) => [l.worktreeKey, l]));

  const lanes: LaneModel[] = laneWorktrees.map((w, column) => {
    const card = cardOf.get(w.key);
    const ready = w.status.state === "ready" ? w.status : null;
    const branchText =
      ready === null || ready.head.kind === "detached" ? w.folder : ready.head.kind === "unborn" ? t("graph.lane.unborn", { name: ready.head.name }) : ready.head.name;
    const noAgent = card === undefined || card.agentState === "none";
    const gl = graphLanes.get(w.key);
    const own = gl?.state.state === "ready" ? gl.state : null;
    return {
      key: w.key,
      column,
      branchText,
      agentLabel: noAgent ? t("graph.noAgent") : card.agent.label,
      colorSlot: card?.agent.colorSlot ?? null,
      muted: noAgent || card.agent.muted,
      symbol: card?.symbol ?? "",
      stateText: card?.stateText ?? "",
      syncText: card?.syncText ?? "",
      shown: own?.commits.length ?? 0,
      total: own?.total ?? null,
    };
  });
  const laneOf = new Map(lanes.map((l) => [l.key, l]));

  const rows: GraphRow[] = [];

  // D7: WIP rows on top, the trunk first, then the lanes in column order.
  const wipFor = (w: WorktreeDto, laneKey: string, branch: string) => {
    const files = filesOf(w);
    if (files === 0) return;
    rows.push({
      kind: "wip",
      id: `wip:${laneKey}`,
      laneKey,
      files,
      a11yLabel: named(laneKey, t("graph.a11y.wip", { branch, count: files })),
    });
  };
  if (trunk !== undefined) wipFor(trunk, "base", baseName ?? "");
  if (state === "ready") for (const w of laneWorktrees) wipFor(w, w.key, laneOf.get(w.key)?.branchText ?? w.folder);

  rows.push({
    kind: "base",
    id: "base",
    name: baseName ?? "",
    a11yLabel: baseName === null ? t("graph.base.none") : t("graph.a11y.base", { base: baseName }),
  });

  if (state !== "ready") return { state, baseName, lanes: [], rows, empty: false, note: null };

  const summaries: GraphRow[] = [];
  const commitEntries: Array<{ lane: LaneModel; index: number; ms: number }> = [];
  const forks = new Map<string, GraphRow[]>();
  const ownCommits = new Map<string, OwnLane>();

  const summary = (lane: LaneModel, note: string) =>
    summaries.push({
      kind: "lane",
      id: `lane:${lane.key}`,
      laneKey: lane.key,
      syncText: lane.syncText,
      note,
      a11yLabel: named(lane.key, t("graph.a11y.lane", { branch: lane.branchText, agent: lane.agentLabel, state: lane.stateText, sync: lane.syncText })),
    });

  for (const w of laneWorktrees) {
    const lane = laneOf.get(w.key);
    if (lane === undefined) continue;
    if (w.status.state === "unavailable") {
      summary(lane, t(`fleet.unavailable.${w.status.reason}`));
      continue;
    }
    if (graph === null) {
      summary(lane, entry?.unavailable ? "" : t("graph.lane.loading"));
      continue;
    }
    const gl = graphLanes.get(w.key);
    if (gl === undefined) {
      summary(lane, graph.cut !== null ? t("graph.lane.cut") : t("graph.lane.loading"));
      continue;
    }
    if (gl.state.state === "not-computable") {
      summary(lane, t(GAP_KEY[gl.state.reason]));
      continue;
    }
    if (gl.state.commits.length === 0) {
      summary(lane, t("graph.lane.gap.noCommits"));
      continue;
    }
    const own = gl.state;
    ownCommits.set(lane.key, own);
    own.commits.forEach((c, index) => commitEntries.push({ lane, index, ms: c.committedUtcMs }));

    const tail: GraphRow[] = [];
    const missing = own.total.count - own.commits.length;
    if (missing > 0) {
      const count = missing;
      const exact = own.total.exact;
      tail.push({
        kind: "more",
        id: `more:${lane.key}`,
        laneKey: lane.key,
        count,
        exact,
        a11yLabel: named(
          lane.key,
          exact ? t("graph.a11y.more", { count, branch: lane.branchText }) : `${t("graph.moreAtLeast", { count })}, ${lane.branchText}`,
        ),
      });
    }
    const commit = own.mergeBase.slice(0, SHA_DIGITS);
    tail.push({
      kind: "fork",
      id: `fork:${lane.key}`,
      laneKey: lane.key,
      mergeBase: own.mergeBase,
      a11yLabel: named(lane.key, t("graph.a11y.fork", { branch: lane.branchText, base: baseName ?? "", commit })),
    });
    forks.set(lane.key, tail);
  }

  // D4: the commits of every lane interleaved by committed time, newest first; ties by column and position.
  commitEntries.sort((a, b) => b.ms - a.ms || a.lane.column - b.lane.column || a.index - b.index);
  const emitted = new Map<string, number>();
  const commitRows: GraphRow[] = [];
  for (const { lane, index } of commitEntries) {
    const own = ownCommits.get(lane.key);
    const c = own?.commits[index];
    if (own === undefined || c === undefined) continue;
    commitRows.push({
      kind: "commit",
      id: `commit:${lane.key}:${c.oid}`,
      laneKey: lane.key,
      oid: c.oid,
      summary: c.summary,
      committedUtcMs: c.committedUtcMs,
      merge: c.merge,
      head: index === 0,
      a11yLabel: named(
        lane.key,
        t("graph.a11y.commit", {
          // The branch is a visible part of the head row only; naming it on every row would repeat it fifty times.
          branch: index === 0 ? lane.branchText : "",
          sha: c.oid.slice(0, SHA_DIGITS),
          message: c.summary,
          author: `${t("graph.author.unattributed")}. ${t("graph.author.tooltip")}`,
          date: formatDate(c.committedUtcMs),
        }),
      ),
    });
    const done = (emitted.get(lane.key) ?? 0) + 1;
    emitted.set(lane.key, done);
    if (done === own.commits.length) commitRows.push(...(forks.get(lane.key) ?? []));
  }

  rows.push(...summaries, ...commitRows);

  const empty =
    lanes.length === 0 ||
    lanes.every((l) => {
      const gl = graphLanes.get(l.key);
      if (gl?.state.state === "ready") return gl.state.total.count === 0;
      const w = laneWorktrees.find((x) => x.key === l.key);
      return w?.status.state === "ready" && w.status.divergence.state === "counted" && w.status.divergence.ahead.count === 0;
    });

  const note: GraphModel["note"] = entry?.unavailable ?? (graph?.cut != null ? "cut" : null);
  return { state, baseName, lanes, rows, empty, note };
}
