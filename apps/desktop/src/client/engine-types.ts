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

export type PairReason =
  | "base-unconfirmed"
  | "operation-in-progress"
  | "power-saving"
  | "dormant"
  | "base-missing"
  | "missing-object"
  | "shallow"
  | "limit-exceeded"
  | "error";

/** The eight prediction limits of `crates/api/src/prediction.rs`, in kebab-case. */
export type LimitCode =
  | "overlap-includes-uncommitted"
  | "conflict-committed-only"
  | "no-merge-drivers-or-attributes"
  | "rename-limit"
  | "insertions-may-merge"
  | "joined-hunks"
  | "pair-ignores-base-moves"
  | "local-only";

export type ConfirmCommandDto =
  | { state: "offered"; command: string; withPath: boolean }
  | { state: "not-offered"; reason: "raptor-unknown" };

export interface LineRangeDto {
  /** 1-based. */
  start: number;
  len: number;
}

export interface ConflictFileDto {
  path: string;
  kind: "content" | "add-add" | "modify-delete" | "rename" | "directory-file" | "binary" | "submodule";
  ours: LineRangeDto[];
  theirs: LineRangeDto[];
  hunksTruncated: boolean;
  hasMergeRule: boolean;
}

export interface PairDto {
  pairId: string;
  rev: number;
  /** Worktree key. */
  a: string;
  /** Worktree key, or `"base"`. */
  b: string;
  state: "calculating" | "current" | "recalculating" | "pending" | "not-computable";
  reason: PairReason | null;
  level: "none" | "overlap" | "conflict" | null;
  computedAtUtcMs: number | null;
  overlapFiles: number;
  overlapPartial: boolean;
  conflictFiles: number;
  conflictTruncated: boolean;
}

export interface PairDetailDto {
  pair: PairDto;
  overlap: string[];
  conflict: { files: ConflictFileDto[]; truncated: boolean } | null;
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
  limits: LimitCode[];
  confirmCommand: ConfirmCommandDto;
}

/** A count that may be a lower bound (`exact: false`); the backend saturates it to `u32`. */
export interface BoundedCountDto {
  count: number;
  exact: boolean;
}

export type GraphBaseGap = "base-unconfirmed" | "no-base" | "base-missing" | "repo-unavailable";
export type LaneGapReason = "worktree-unavailable" | "no-commits" | "no-merge-base" | "too-slow" | "unreadable";

export interface CommitDto {
  /** 40 or 64 hex digits; the backend drops a commit with any other oid. */
  oid: string;
  summary: string;
  committedUtcMs: number;
  merge: boolean;
}

/** One lane of `graph.lanes`, keyed by the worktree key (DS-US-DSK-005 § Contratos compartidos). */
export interface LaneDto {
  worktreeKey: string;
  head: { kind: "branch"; name: string } | { kind: "detached" };
  state:
    | { state: "ready"; mergeBase: string; total: BoundedCountDto; commits: CommitDto[] }
    | { state: "not-computable"; reason: LaneGapReason };
}

export interface GraphDto {
  computedUtcMs: number;
  base: { state: "ready"; name: string } | { state: "not-computable"; reason: GraphBaseGap; name: string | null };
  lanes: LaneDto[];
  lanesTotal: number;
  cut: "lanes" | "bytes" | null;
}

export type GraphUnavailableReason = "unsupported" | "failed";

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
  details: Map<string, PairDetailDto>;
  lastComputed: Map<string, number>;
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
  | { type: "repoSession"; repoId: string; session: SessionDto; perf: FleetPerf }
  | { type: "prediction"; repoId: string; pairs: PairDto[]; removed: string[]; perf: FleetPerf }
  | { type: "pairDetails"; repoId: string; details: PairDetailDto[]; filled: boolean; perf: FleetPerf }
  | { type: "graph"; repoId: string; graph: GraphDto; perf: FleetPerf }
  | { type: "graphUnavailable"; repoId: string; reason: GraphUnavailableReason }
  | { type: "guardBlocks"; blocks: GuardBlocksDto }
  | { type: "guardBlocksUnavailable"; repoId: string; cause: GuardUnavailableCause; protection: ProtectionDto | null }
  | { type: "repoClosed"; repoId: string };

export interface ProtectionDto {
  state: "unprotected" | "hooks-only" | "mcp-only" | "full";
}
export type GuardUnavailableCause = "log-unavailable" | "not-observed" | "refused" | "not-open";
export interface RefDto {
  name: string;
  change: "create" | "update" | "force" | "delete";
}
export type OperationDto =
  | { kind: "push"; remote: string | null; refs: RefDto[] }
  | { kind: "ref-transaction"; refs: RefDto[] }
  | { kind: "rebase" }
  | { kind: "commit" };
export type NotOffered =
  | "commit"
  | "rebase"
  | "operation"
  | "rule"
  | "remote"
  | "where-unknown"
  | "rate-limited"
  | "unsafe-text"
  | "raptor-unknown";
export type ExceptionCommandDto = { state: "offered"; command: string } | { state: "not-offered"; reason: NotOffered };
export interface BlockEntryDto {
  decisionId: string;
  kind: "denial" | "exception" | "exception-cancelled" | "exception-rejected";
  atMs: number;
  lastMs: number;
  count: number;
  actor: { kind: "claude-code" } | { kind: "other" } | null;
  worktree: { folder: string; path: string } | null;
  branch: string | null;
  operation: OperationDto;
  rules: { rule: string; level: string; cause: string | null }[];
  layer: "hooks" | "mcp" | "cockpit";
  detail: "full" | "rate-limited";
  exception: ExceptionCommandDto | null;
}
export interface PendingExceptionDto {
  appliesAtMs: number;
  windowMs: number;
  operation: OperationDto;
  rules: string[];
  cancelCommand: string | null;
}
export interface GuardBlocksDto {
  repoId: string;
  protection: ProtectionDto;
  sinceMs: number;
  summary: { blocked: number; exceptions: number; exceptionsRejected: number };
  entries: BlockEntryDto[];
  pending: PendingExceptionDto | null;
  unlogged: { fromMs: number; toMs: number | null }[];
}
