import type { QueryClient } from "@tanstack/react-query";
import type {
  BlockEntryDto,
  GuardBlocksDto,
  GuardUnavailableCause,
  ProtectionDto,
  CandidateDto,
  EngineMessage,
  GlobalDto,
  GraphDto,
  GraphUnavailableReason,
  RepoFleet,
  SessionDto,
} from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { useEngineStore } from "./engine-store";
import { useUiStore } from "./ui-store";

/** Guard blocks of the open repo; `unavailable` keeps the last entries (DS-US-DSK-007 D13). */
export type GuardBlocks =
  | ({ state: "ready" } & GuardBlocksDto)
  | {
      state: "unavailable";
      cause: GuardUnavailableCause;
      protection: ProtectionDto | null;
      entries: BlockEntryDto[];
      readAtMs: number | null;
    };

/** The graph of the open repo: the last `graph.lanes` result, and why it could not be refreshed (DS-US-DSK-005 D17). */
export interface GraphEntry {
  graph: GraphDto | null;
  unavailable: GraphUnavailableReason | null;
}

/** When the last `guardBlocks` of each repo arrived, to tell how old the entries of an unavailable read are. */
const lastGuardRead = new Map<string, number>();

/** The global entry: the last snapshot, or the restricted view of D6 (`restricted`, no repos). */
export type GlobalEntry = GlobalDto & { restricted?: true };

/** D3 merge: the later `stateSinceUtcMs` wins; on a tie the ended one; otherwise the incoming one. */
function mergeSession(old: SessionDto | undefined, incoming: SessionDto): SessionDto {
  if (old === undefined) return incoming;
  if (old.stateSinceUtcMs !== incoming.stateSinceUtcMs) return old.stateSinceUtcMs > incoming.stateSinceUtcMs ? old : incoming;
  if (old.state === "ended" && incoming.state !== "ended") return old;
  return incoming;
}

/**
 * Folds one batch of channel messages into the app state, in order: `conn` and `session` go to the engine store,
 * the data variants to the query cache. Always a fresh object, never a mutation.
 */
export function applyEngineBatch(client: QueryClient, batch: readonly EngineMessage[]): void {
  useEngineStore.getState().apply(batch);

  const patchGlobal = (patch: (global: GlobalEntry) => GlobalEntry) =>
    client.setQueryData<GlobalEntry>(engineKeys.global, (old) => (old === undefined ? old : patch(old)));
  const patchRepo = (repoId: string, patch: (repo: GlobalDto["repos"][number]) => GlobalDto["repos"][number]) =>
    patchGlobal((g) => ({ ...g, repos: g.repos.map((r) => (r.repoId === repoId ? patch(r) : r)) }));

  const patchFleet = (repoId: string, patch: (fleet: RepoFleet) => RepoFleet) =>
    client.setQueryData<RepoFleet>(engineKeys.repo(repoId), (old) => (old === undefined ? old : patch(old)));

  for (const message of batch) {
    switch (message.type) {
      case "global":
        useEngineStore.getState().clearClosedRepos();
        client.setQueryData<GlobalEntry>(engineKeys.global, {
          engine: message.engine,
          daemon: message.daemon,
          autostart: message.autostart,
          repos: message.repos,
        });
        break;
      case "engineState":
        patchGlobal((g) => ({ ...g, engine: message.engine }));
        break;
      case "repoObserved":
        if (!client.getQueryData<GlobalEntry>(engineKeys.global)?.repos.some((r) => r.repoId === message.repo.repoId)) {
          useEngineStore.getState().clearClosedRepos();
        }
        patchGlobal((g) => ({
          ...g,
          repos: g.repos.some((r) => r.repoId === message.repo.repoId)
            ? g.repos.map((r) => (r.repoId === message.repo.repoId ? message.repo : r))
            : [...g.repos, message.repo],
        }));
        break;
      case "repoRetired":
        useEngineStore.getState().clearClosedRepos();
        patchGlobal((g) => ({ ...g, repos: g.repos.filter((r) => r.repoId !== message.repoId) }));
        break;
      case "repoTier":
        patchRepo(message.repoId, (r) => ({ ...r, tier: message.tier }));
        break;
      case "repoAttention":
        patchRepo(message.repoId, (r) => ({ ...r, attention: message.attention }));
        break;
      case "discovered":
        client.setQueryData<CandidateDto[]>(engineKeys.discovered, (old) =>
          message.replace || old === undefined ? message.candidates : [...old, ...message.candidates],
        );
        break;
      case "engineOnly":
        client.setQueryData<GlobalEntry>(engineKeys.global, (old) => ({
          engine: message.engine,
          daemon: old?.daemon ?? { version: "", protocol: 0 },
          autostart: old?.autostart ?? "unknown",
          repos: [],
          restricted: true,
        }));
        break;
      case "repo":
        client.setQueryData<RepoFleet>(engineKeys.repo(message.fleet.repoId), (old) => {
          const inSnapshot = new Set(message.fleet.pairs.map((p) => p.pairId));
          const lastComputed = new Map([...(old?.lastComputed ?? [])].filter(([id]) => inSnapshot.has(id)));
          return { repo: message.fleet, sessions: new Map(), detection: null, details: new Map(), lastComputed };
        });
        break;
      case "worktrees":
        patchFleet(message.repoId, (f) => ({
          ...f,
          repo: { ...f.repo, worktrees: message.worktrees, fetchedUtcMs: message.fetchedUtcMs },
        }));
        break;
      case "sessions":
        patchFleet(message.repoId, (f) => {
          const sessions = new Map(f.sessions);
          for (const s of message.sessions) sessions.set(s.sessionId, mergeSession(sessions.get(s.sessionId), s));
          return { ...f, sessions, detection: message.detectionAvailable };
        });
        break;
      case "repoSession":
        patchFleet(message.repoId, (f) => {
          const sessions = new Map(f.sessions);
          sessions.set(message.session.sessionId, mergeSession(sessions.get(message.session.sessionId), message.session));
          return { ...f, sessions };
        });
        break;
      case "prediction":
        patchFleet(message.repoId, (f) => {
          const removed = new Set(message.removed);
          const lastComputed = new Map(f.lastComputed);
          const details = new Map(f.details);
          for (const id of removed) {
            lastComputed.delete(id);
            details.delete(id);
          }
          const byId = new Map(f.repo.pairs.filter((p) => !removed.has(p.pairId)).map((p) => [p.pairId, p]));
          for (const incoming of message.pairs) {
            if (removed.has(incoming.pairId)) continue;
            const stored = byId.get(incoming.pairId);
            if (stored !== undefined && incoming.rev < stored.rev) continue;
            if (stored?.computedAtUtcMs != null && incoming.computedAtUtcMs == null) lastComputed.set(incoming.pairId, stored.computedAtUtcMs);
            byId.set(incoming.pairId, incoming);
          }
          return { ...f, repo: { ...f.repo, pairs: [...byId.values()] }, details, lastComputed };
        });
        break;
      case "pairDetails":
        patchFleet(message.repoId, (f) => {
          const byId = new Map(f.repo.pairs.map((p) => [p.pairId, p]));
          const details = new Map(f.details);
          const lastComputed = new Map(f.lastComputed);
          for (const detail of message.details) {
            const id = detail.pair.pairId;
            const stored = byId.get(id);
            if (stored !== undefined && detail.pair.rev < stored.rev) continue;
            if (stored === undefined || detail.pair.rev > stored.rev) {
              if (stored?.computedAtUtcMs != null && detail.pair.computedAtUtcMs == null) lastComputed.set(id, stored.computedAtUtcMs);
              byId.set(id, detail.pair);
            }
            details.set(id, detail);
          }
          const pairsTruncated = message.filled ? false : f.repo.pairsTruncated;
          return { ...f, repo: { ...f.repo, pairs: [...byId.values()], pairsTruncated }, details, lastComputed };
        });
        break;
      case "graph":
        client.setQueryData<GraphEntry>(engineKeys.graph(message.repoId), { graph: message.graph, unavailable: null });
        break;
      case "graphUnavailable":
        // The last graph stays: the view keeps the commits it has and says it could not refresh them.
        client.setQueryData<GraphEntry>(engineKeys.graph(message.repoId), (old) => ({ graph: old?.graph ?? null, unavailable: message.reason }));
        break;
      case "guardBlocks":
        lastGuardRead.set(message.blocks.repoId, Date.now());
        client.setQueryData<GuardBlocks>(engineKeys.guard(message.blocks.repoId), { state: "ready", ...message.blocks });
        break;
      case "guardBlocksUnavailable": {
        if (message.cause === "not-open" && useUiStore.getState().selectedRepoId !== message.repoId) break;
        client.setQueryData<GuardBlocks>(engineKeys.guard(message.repoId), (old) => ({
          state: "unavailable",
          cause: message.cause,
          protection: message.protection,
          entries: old?.entries ?? [],
          readAtMs: old === undefined ? null : old.state === "unavailable" ? old.readAtMs : (lastGuardRead.get(message.repoId) ?? null),
        }));
        break;
      }
      case "repoClosed":
        lastGuardRead.delete(message.repoId);
        client.removeQueries({ queryKey: engineKeys.guard(message.repoId), exact: true });
        useEngineStore.getState().markRepoClosed(message.repoId);
        client.removeQueries({ queryKey: engineKeys.repo(message.repoId), exact: true });
        client.removeQueries({ queryKey: engineKeys.graph(message.repoId), exact: true });
        if (useUiStore.getState().selectedRepoId === message.repoId) useUiStore.getState().selectRepo(null);
        break;
      case "conn":
      case "session":
        break;
    }
  }
}
