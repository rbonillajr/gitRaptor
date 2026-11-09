import type { QueryClient } from "@tanstack/react-query";
import type { CandidateDto, EngineMessage, GlobalDto, RepoFleet, SessionDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { useEngineStore } from "./engine-store";
import { useUiStore } from "./ui-store";

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
        client.setQueryData<RepoFleet>(engineKeys.repo(message.fleet.repoId), {
          repo: message.fleet,
          sessions: new Map(),
          detection: null,
          details: new Map(),
          lastComputed: new Map(),
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
          const incoming = new Map(message.pairs.map((p) => [p.pairId, p]));
          const removed = new Set(message.removed);
          const kept = f.repo.pairs.filter((p) => !removed.has(p.pairId)).map((p) => incoming.get(p.pairId) ?? p);
          const known = new Set(f.repo.pairs.map((p) => p.pairId));
          const added = message.pairs.filter((p) => !known.has(p.pairId) && !removed.has(p.pairId));
          return { ...f, repo: { ...f.repo, pairs: [...kept, ...added] } };
        });
        break;
      case "repoClosed":
        useEngineStore.getState().markRepoClosed(message.repoId);
        client.removeQueries({ queryKey: engineKeys.repo(message.repoId), exact: true });
        if (useUiStore.getState().selectedRepoId === message.repoId) useUiStore.getState().selectRepo(null);
        break;
      case "conn":
      case "session":
      case "pairDetails":
        break;
    }
  }
}
