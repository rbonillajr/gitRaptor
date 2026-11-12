import { create } from "zustand";
import type { AlertState } from "@/conflicts/alerts";
import type { ConnState, EngineMessage, ResyncReason, SessionInfo } from "@/client/engine-types";

/** What the engine connection looks like right now. Not persisted: it authorizes nothing (ADR-GRP-017 § 1). */
export interface EngineState {
  conn: ConnState;
  staleSinceMs: number | null;
  resync: ResyncReason | null;
  session: SessionInfo | null;
  lastRepoBatch: RepoBatchMark | null;
  /** The last batch that carried a `graph` message: `GraphView` measures `gr:graph-delta` from `receivedAt` (T009). */
  lastGraphBatch: RepoBatchMark | null;
  /** Repos the backend closed (`repoClosed`); skipped by the default choice until the repo list changes. */
  closedRepoIds: ReadonlySet<string>;
  /** The conflicts each repo already knows, so a conflict alerts once (DS-US-DSK-008 D12). Lives all run, in memory. */
  alertState: AlertState;
  /** `attention.conflicts` last seen per repo; `null` until the first global snapshot seeds it (D15). */
  seededCounts: ReadonlyMap<string, number> | null;
}

/** The last batch that carried repo messages: `FleetView` measures `gr:fleet-delta` from `receivedAt` (T006). */
export interface RepoBatchMark {
  receivedAt: number;
  n: number;
}

export interface EngineActions {
  noteRepoBatch: (mark: RepoBatchMark) => void;
  noteGraphBatch: (mark: RepoBatchMark) => void;
  markRepoClosed: (repoId: string) => void;
  clearClosedRepos: () => void;
  setRepoAlerts: (repoId: string, known: ReadonlyMap<string, "seeded" | "alerted"> | null) => void;
  setSeededCounts: (counts: ReadonlyMap<string, number>) => void;
  /** Folds a batch of channel messages into one state update (one render per frame). */
  apply: (batch: readonly EngineMessage[]) => void;
}

export const useEngineStore = create<EngineState & EngineActions>()((set) => ({
  conn: { kind: "connecting" },
  staleSinceMs: null,
  resync: null,
  session: null,
  lastRepoBatch: null,
  lastGraphBatch: null,
  closedRepoIds: new Set<string>(),
  alertState: new Map(),
  seededCounts: null,
  setRepoAlerts: (repoId, known) =>
    set((s) => {
      const alertState = new Map(s.alertState);
      if (known === null || known.size === 0) alertState.delete(repoId);
      else alertState.set(repoId, known);
      return { alertState };
    }),
  setSeededCounts: (seededCounts) => set({ seededCounts }),
  markRepoClosed: (repoId) => set((s) => ({ closedRepoIds: new Set(s.closedRepoIds).add(repoId) })),
  clearClosedRepos: () => set((s) => (s.closedRepoIds.size === 0 ? s : { closedRepoIds: new Set<string>() })),
  noteRepoBatch: (mark) => set({ lastRepoBatch: mark }),
  noteGraphBatch: (mark) => set({ lastGraphBatch: mark }),
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
