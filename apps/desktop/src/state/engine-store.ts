import { create } from "zustand";
import type { ConnState, EngineMessage, ResyncReason, SessionInfo } from "@/client/engine-types";

/** What the engine connection looks like right now. Not persisted: it authorizes nothing (ADR-GRP-017 § 1). */
export interface EngineState {
  conn: ConnState;
  staleSinceMs: number | null;
  resync: ResyncReason | null;
  session: SessionInfo | null;
  lastRepoBatch: RepoBatchMark | null;
  /** Repos the backend closed (`repoClosed`); skipped by the default choice until the repo list changes. */
  closedRepoIds: ReadonlySet<string>;
}

/** The last batch that carried repo messages: `FleetView` measures `gr:fleet-delta` from `receivedAt` (T006). */
export interface RepoBatchMark {
  receivedAt: number;
  n: number;
}

export interface EngineActions {
  noteRepoBatch: (mark: RepoBatchMark) => void;
  markRepoClosed: (repoId: string) => void;
  clearClosedRepos: () => void;
  /** Folds a batch of channel messages into one state update (one render per frame). */
  apply: (batch: readonly EngineMessage[]) => void;
}

export const useEngineStore = create<EngineState & EngineActions>()((set) => ({
  conn: { kind: "connecting" },
  staleSinceMs: null,
  resync: null,
  session: null,
  lastRepoBatch: null,
  closedRepoIds: new Set<string>(),
  markRepoClosed: (repoId) => set((s) => ({ closedRepoIds: new Set(s.closedRepoIds).add(repoId) })),
  clearClosedRepos: () => set((s) => (s.closedRepoIds.size === 0 ? s : { closedRepoIds: new Set<string>() })),
  noteRepoBatch: (mark) => set({ lastRepoBatch: mark }),
  apply: (batch) =>
    set((state) => {
      let next: Partial<EngineState> = {};
      for (const message of batch) {
        if (message.type === "conn") {
          next = { ...next, conn: message.state, staleSinceMs: message.staleSinceMs, resync: message.resync };
        } else if (message.type === "session") {
          next = {
            ...next,
            session: {
              daemonVersion: message.daemonVersion,
              protocol: message.protocol,
              restricted: message.restricted,
              activity: message.activity,
              discovery: message.discovery,
            },
          };
        }
      }
      return Object.keys(next).length > 0 ? next : state;
    }),
}));
