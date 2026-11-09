import { skipToken, useQuery, type UseQueryResult } from "@tanstack/react-query";
import { useEffect } from "react";
import type { RepoFleet, RepoSummaryDto } from "@/client/engine-types";
import { call } from "@/client/ipc";
import { engineKeys } from "@/client/queries";
import type { GlobalEntry } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";

/** The default open repo (D2): the first openable one (not unavailable, not excluded) by name, then `repoId`. */
export function defaultRepoId(repos: readonly RepoSummaryDto[], excluded: ReadonlySet<string> = new Set()): string | null {
  const first = [...repos]
    .filter((r) => r.state !== "unavailable" && !excluded.has(r.repoId))
    .sort((a, b) => a.name.localeCompare(b.name) || a.repoId.localeCompare(b.repoId))[0];
  return first?.repoId ?? null;
}

/**
 * The open repo of the fleet. Mount it once (`EngineCenter`): it picks the default repo (first by name, then
 * `repoId`) when the global snapshot arrives with none chosen or the chosen one disappears, asks the backend to open
 * the repo on every change of `selectedRepoId`, and returns the cache entry of that repo. The entry has no
 * `queryFn`: only `applyEngineBatch` writes it.
 */
export function useFleet(): UseQueryResult<RepoFleet> {
  const selected = useUiStore((s) => s.selectedRepoId);
  const selectRepo = useUiStore((s) => s.selectRepo);
  const global = useQuery<GlobalEntry>({ queryKey: engineKeys.global, queryFn: skipToken }).data;
  const repos = global?.repos;
  const closed = useEngineStore((s) => s.closedRepoIds);

  useEffect(() => {
    if (repos === undefined) return;
    if (selected !== null && repos.some((r) => r.repoId === selected)) return;
    // The open repo was closed or disappeared: back to the previous one if it is still openable, else the default.
    const previousId = useUiStore.getState().previousRepoId;
    const previous = repos.find((r) => r.repoId === previousId && r.state !== "unavailable" && !closed.has(r.repoId));
    const next = previous?.repoId ?? defaultRepoId(repos, closed);
    if (next !== selected) selectRepo(next);
  }, [repos, selected, selectRepo, closed]);

  useEffect(() => {
    if (selected === null) return;
    call("fleet_open", { repoId: selected }).catch(() => undefined);
  }, [selected]);

  return useQuery<RepoFleet>({
    queryKey: engineKeys.repo(selected ?? ""),
    queryFn: skipToken,
    enabled: selected !== null,
  });
}
