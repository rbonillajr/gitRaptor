/**
 * TS mirror of the engine wire types (`apps/desktop/src-tauri/src/engine/wire.rs`, DS-US-DSK-003 § Mensajes del
 * `Channel`). Types only. Every `string` that comes from the file system or an agent is already sanitized by
 * `visible` on the Rust side; the UI still renders it as React text and nothing else.
 */

export type ConnState =
  | { kind: "connecting" }
  | { kind: "starting" }
  | { kind: "syncing" }
  | { kind: "live" }
  | { kind: "reconnecting"; attempt: number; retryInMs: number }
  | { kind: "stopped"; reason: "not-running" | "launch-failed" }
  | {
      kind: "incompatible";
      which: "daemon-older" | "app-older" | "missing-capabilities";
      daemonVersion: string | null;
      daemonPath: string | null;
      stopCommand: string | null;
      action: "start" | "copy-stop" | "update-app" | "none";
    }
  | { kind: "rejected" }
  | { kind: "unsupported" };

export type ResyncReason = "slow-consumer" | "replay-unavailable" | "daemon-restarted" | "scope-closed";

export type Tier = "active" | "waking" | "dormant";

export interface SessionInfo {
  daemonVersion: string;
  protocol: number;
  restricted: boolean;
  activity: boolean;
  discovery: boolean;
}

export interface EngineDto {
  state: "waiting-for-git" | "no-repos" | "observing";
  gitVersion: string | null;
}

export type CountDto = { state: "counted"; count: number } | { state: "unavailable" };

export interface AttentionDto {
  conflicts: CountDto;
  denials: CountDto;
}

export interface RepoSummaryDto {
  repoId: string;
  name: string;
  path: string;
  state: "observed" | "unavailable";
  tier: Tier | null;
  attention: AttentionDto;
}

export interface CandidateDto {
  name: string;
  path: string;
}

export interface GlobalDto {
  engine: EngineDto;
  daemon: { version: string; protocol: number };
  autostart: "registered" | "not-registered" | "unknown";
  repos: RepoSummaryDto[];
}

export type BaseStatus = "confirmed" | "unconfirmed" | "invalid";

export interface ReadyDto {
  state: "ready";
  head: { kind: "branch" | "unborn"; name: string } | { kind: "detached" };
  counts: { staged: number; unstaged: number; untracked: number };
  divergence:
    | { state: "counted"; ahead: { count: number; exact: boolean }; behind: { count: number; exact: boolean } }
    | { state: "base-missing" | "no-base" | "no-commits" | "unreadable" };
}

export type UnavailableReason = "missing" | "untrusted" | "unreadable";

/** One working copy. `key` is the FNV-1a hash of the raw root path: it pairs sessions and pairs with the card. */
export interface WorktreeDto {
  key: string;
  folder: string;
  path: string;
  main: boolean;
  status: ReadyDto | { state: "unavailable"; reason: UnavailableReason };
  lastActivityUtcMs: number | null;
  lastActivityInGap: boolean;
  /** Full commit hash of a detached HEAD; the UI cuts it to 7 digits. */
  detachedAt: string | null;
  operation: string | null;
}

export type AgentDto = { kind: "claude-code" } | { kind: "other"; name: string };
export type SessionState = "active" | "inactive" | "ended";
export type EndCause = "process-gone" | "ended-during-gap" | "registration-withdrawn" | "worktree-removed";

export interface SessionDto {
  sessionId: string;
  worktreeKey: string;
  agent: AgentDto;
  origin: "detected" | "registered";
  state: SessionState;
  startedUtcMs: number;
  stateSinceUtcMs: number;
  endedUtcMs: number | null;
  endCause: EndCause | null;
}

export interface PairDto {
  pairId: string;
  /** Worktree key. */
  a: string;
  /** Worktree key, or `"base"`. */
  b: string;
  state: "calculating" | "current" | "recalculating" | "pending" | "not-computable";
  level: "none" | "overlap" | "conflict" | null;
}

export interface RepoFleetDto {
  repoId: string;
  name: string;
  base: { name: string | null; status: BaseStatus };
  fetchedUtcMs: number | null;
  tier: Tier | null;
  worktrees: WorktreeDto[];
  pairs: PairDto[];
  pairsTruncated: boolean;
}

/** Backend time spent on a delta (DS-US-DSK-004 T006). */
export interface FleetPerf {
  backendNs: number;
}

/** Cache entry of `engineKeys.repo(id)`: the repo snapshot plus the sessions merged by `sessionId`. */
export interface RepoFleet {
  repo: RepoFleetDto;
  sessions: Map<string, SessionDto>;
  /** `null` until `sessions.list` answers. */
  detection: boolean | null;
}

export type EngineMessage =
  | { type: "conn"; state: ConnState; staleSinceMs: number | null; resync: ResyncReason | null }
  | ({ type: "session" } & SessionInfo)
  | ({ type: "global" } & GlobalDto)
  | { type: "engineState"; engine: EngineDto }
  | { type: "repoObserved"; repo: RepoSummaryDto }
  | { type: "repoRetired"; repoId: string }
  | { type: "repoTier"; repoId: string; tier: Tier }
  | { type: "repoAttention"; repoId: string; attention: AttentionDto }
  | { type: "discovered"; candidates: CandidateDto[]; replace: boolean }
  | { type: "engineOnly"; engine: EngineDto }
  | { type: "repo"; fleet: RepoFleetDto }
  | { type: "worktrees"; repoId: string; worktrees: WorktreeDto[]; fetchedUtcMs: number | null; perf: FleetPerf }
  | { type: "sessions"; repoId: string; detectionAvailable: boolean; sessions: SessionDto[] }
  | { type: "session"; repoId: string; session: SessionDto; perf: FleetPerf }
  | { type: "prediction"; repoId: string; pairs: PairDto[]; removed: string[]; perf: FleetPerf }
  | { type: "repoClosed"; repoId: string };
