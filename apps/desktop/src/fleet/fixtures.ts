// Test fixtures for the fleet view model and components (not shipped: only the tests import it).
import type { PairDto, ReadyDto, RepoFleet, RepoFleetDto, SessionDto, WorktreeDto } from "@/client/engine-types";

export const NOW = Date.UTC(2026, 10, 9, 12, 0, 0);
export const MIN = 60_000;
export const HOUR = 60 * MIN;

export const ready = (over: Partial<ReadyDto> = {}): ReadyDto => ({
  state: "ready",
  head: { kind: "branch", name: "main" },
  counts: { staged: 0, unstaged: 0, untracked: 0 },
  divergence: { state: "counted", ahead: { count: 0, exact: true }, behind: { count: 0, exact: true } },
  ...over,
});

/** `exact` applies to both sides, or `[ahead, behind]` per side. */
export const counted = (ahead: number, behind: number, exact: boolean | [boolean, boolean] = true): ReadyDto["divergence"] => {
  const [a, b] = typeof exact === "boolean" ? [exact, exact] : exact;
  return { state: "counted", ahead: { count: ahead, exact: a }, behind: { count: behind, exact: b } };
};

export const worktree = (folder: string, over: Partial<WorktreeDto> = {}): WorktreeDto => ({
  key: `key-${folder}`,
  folder,
  path: `/w/tienda/${folder}`,
  main: false,
  status: ready({ head: { kind: "branch", name: folder } }),
  lastActivityUtcMs: NOW - 2 * MIN,
  lastActivityInGap: false,
  detachedAt: null,
  operation: null,
  ...over,
});

export const session = (id: string, folder: string, over: Partial<SessionDto> = {}): SessionDto => ({
  sessionId: id,
  worktreeKey: `key-${folder}`,
  agent: { kind: "claude-code" },
  origin: "detected",
  state: "active",
  startedUtcMs: NOW - HOUR,
  stateSinceUtcMs: NOW - 5 * MIN,
  endedUtcMs: null,
  endCause: null,
  ...over,
});

export const pair = (a: string, b: string, over: Partial<PairDto> = {}): PairDto => ({
  pairId: `${a}~${b}`,
  a: a === "base" ? a : `key-${a}`,
  b: b === "base" ? b : `key-${b}`,
  state: "current",
  level: "conflict",
  ...over,
});

export interface FleetInput {
  worktrees?: WorktreeDto[];
  sessions?: SessionDto[];
  pairs?: PairDto[];
  detection?: boolean | null;
  repo?: Partial<RepoFleetDto>;
}

/** The "tienda" repo: main, feat-pagos (↑3 ↓1, 2 files) and feat-login (↑1 ↓0), two active Claude Code sessions. */
export function tienda(input: FleetInput = {}): RepoFleet {
  const worktrees = input.worktrees ?? [
    worktree("tienda", { main: true, path: "/w/tienda", status: ready() }),
    worktree("feat-pagos", { status: ready({ head: { kind: "branch", name: "feat-pagos" }, counts: { staged: 1, unstaged: 1, untracked: 0 }, divergence: counted(3, 1) }) }),
    worktree("feat-login", { status: ready({ head: { kind: "branch", name: "feat-login" }, divergence: counted(1, 0) }) }),
  ];
  const sessions = input.sessions ?? [session("claude-1", "feat-pagos"), session("claude-2", "feat-login")];
  return {
    repo: {
      repoId: "tienda",
      name: "tienda",
      base: { name: "main", status: "confirmed" },
      fetchedUtcMs: NOW - 3 * HOUR,
      tier: "active",
      worktrees,
      pairs: input.pairs ?? [],
      pairsTruncated: false,
      ...input.repo,
    },
    sessions: new Map(sessions.map((s) => [s.sessionId, s])),
    detection: input.detection === undefined ? true : input.detection,
  };
}
