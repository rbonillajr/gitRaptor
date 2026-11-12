// Test fixtures of the lane graph (not shipped: only the tests import it). They build on the "tienda" script of
// DS-US-DSK-004 (`src/fleet/fixtures.ts`): main (trunk, 3 uncommitted files), feat-pagos (↑3 ↓1, 2 files, claude-1)
// and feat-login (↑1 ↓0, clean, claude-2). Commit subjects are invented; there is never an author.
import i18n from "i18next";
import type { BoundedCountDto, CommitDto, GraphDto, LaneDto, LaneGapReason, RepoFleet } from "@/client/engine-types";
import { fleetCards, type CardModel } from "@/fleet/fleet-model";
import { MIN, NOW, counted, ready, tienda, worktree } from "@/fleet/fixtures";
import { graphModel } from "./graph-model";
import type { GraphEntry, GraphModel } from "./graph-types";

export { HOUR, MIN, NOW, counted, pair, ready, session, tienda, worktree } from "@/fleet/fixtures";

export const MERGE_BASE = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

const hex = (n: number) => n.toString(16).padEnd(40, "0");

export const commit = (n: number, summary: string, committedUtcMs: number, over: Partial<CommitDto> = {}): CommitDto => ({
  oid: hex(n),
  summary,
  committedUtcMs,
  merge: false,
  ...over,
});

/** `n` commits of one lane, newest first, one minute apart from `fromMs` backwards. */
export const commits = (n: number, prefix: string, fromMs: number, firstOid = 1000): CommitDto[] =>
  Array.from({ length: n }, (_, i) => commit(firstOid + i, `${prefix} ${n - i}`, fromMs - i * MIN));

export const readyLane = (folder: string, list: CommitDto[], total: BoundedCountDto = { count: list.length, exact: true }, mergeBase = MERGE_BASE): LaneDto => ({
  worktreeKey: `key-${folder}`,
  head: { kind: "branch", name: folder },
  state: { state: "ready", mergeBase, total, commits: list },
});

export const gapLane = (folder: string, reason: LaneGapReason): LaneDto => ({
  worktreeKey: `key-${folder}`,
  head: { kind: "branch", name: folder },
  state: { state: "not-computable", reason },
});

/** pagos newest at -10 min, login at -20 min, pagos again at -30 and -50 min: interleaved they read p1 l1 p2 p3. */
export const PAGOS = [
  commit(1, "Add the payment form", NOW - 10 * MIN),
  commit(2, "Validate the card number", NOW - 30 * MIN),
  commit(3, "Scaffold the payments page", NOW - 50 * MIN),
];
export const LOGIN = [commit(4, "Add the login form", NOW - 20 * MIN)];

/** The graph of "tienda": the commits of its two lanes. */
export function tiendaGraph(over: Partial<GraphDto> = {}): GraphDto {
  return {
    computedUtcMs: NOW,
    base: { state: "ready", name: "main" },
    lanes: [readyLane("feat-pagos", PAGOS, { count: 3, exact: true }), readyLane("feat-login", LOGIN, { count: 1, exact: true })],
    lanesTotal: 2,
    cut: null,
    ...over,
  };
}

/** "tienda" plus a third lane, `feat-lento`, whose commits took too long. */
export function tiendaWithSlowLane(): { fleet: RepoFleet; graph: GraphDto } {
  const base = tienda();
  const fleet = tienda({
    worktrees: [...base.repo.worktrees, worktree("feat-lento", { status: ready({ head: { kind: "branch", name: "feat-lento" }, divergence: counted(5, 0) }) })],
  });
  return { fleet, graph: tiendaGraph({ lanes: [...tiendaGraph().lanes, gapLane("feat-lento", "too-slow")], lanesTotal: 3 }) };
}

/** "tienda-develop": base `develop` confirmed, but no worktree has it (`base-missing`). */
export function tiendaDevelop(): RepoFleet {
  const fleet = tienda();
  return tienda({
    worktrees: fleet.repo.worktrees.map((w) =>
      w.status.state === "ready" ? { ...w, status: { ...w.status, divergence: { state: "base-missing" as const } } } : w,
    ),
    repo: { base: { name: "develop", status: "confirmed" } },
  });
}

/** "tienda-sin-confirmar": the base is resolved but nobody confirmed it. */
export const tiendaUnconfirmed = (): RepoFleet => tienda({ repo: { base: { name: "main", status: "unconfirmed" } } });

/** A repo whose base is invalid: no name at all. */
export const tiendaNoBase = (): RepoFleet => tienda({ repo: { base: { name: null, status: "invalid" } } });

export const entryOf = (graph: GraphDto | null, unavailable: GraphEntry["unavailable"] = null): GraphEntry => ({ graph, unavailable });

/** The cards the way the graph reads them, with colour slots 0, 1, … assigned to the sessions in order. */
export function cardsOf(fleet: RepoFleet): CardModel[] {
  const slots = new Map([...fleet.sessions.values()].map((s, i) => [s.sessionId, i]));
  return fleetCards(fleet, NOW, true, i18n.t).map((c) => (c.agent.sessionId === null ? c : { ...c, agent: { ...c.agent, colorSlot: slots.get(c.agent.sessionId) ?? null } }));
}

export function modelOf(fleet: RepoFleet | undefined, entry: GraphEntry | null = null, laneA11y?: (key: string) => string | null): GraphModel {
  return graphModel({
    fleet,
    cards: fleet ? cardsOf(fleet) : [],
    entry,
    t: i18n.t,
    locale: i18n.language,
    ...(laneA11y ? { laneA11y } : {}),
  });
}
