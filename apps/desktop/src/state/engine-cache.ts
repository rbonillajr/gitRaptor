import type { QueryClient } from "@tanstack/react-query";
import type { CandidateDto, EngineMessage, GlobalDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { useEngineStore } from "./engine-store";

/** The global entry: the last snapshot, or the restricted view of D6 (`restricted`, no repos). */
export type GlobalEntry = GlobalDto & { restricted?: true };

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

  for (const message of batch) {
    switch (message.type) {
      case "global":
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
        patchGlobal((g) => ({
          ...g,
          repos: g.repos.some((r) => r.repoId === message.repo.repoId)
            ? g.repos.map((r) => (r.repoId === message.repo.repoId ? message.repo : r))
            : [...g.repos, message.repo],
        }));
        break;
      case "repoRetired":
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
      case "conn":
      case "session":
        break;
    }
  }
}
