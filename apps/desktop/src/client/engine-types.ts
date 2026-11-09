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
  | { type: "engineOnly"; engine: EngineDto };
