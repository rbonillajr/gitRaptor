import { skipToken, useQuery, type UseQueryResult } from "@tanstack/react-query";
import { useEffect } from "react";
import type { RepoFleet, RepoSummaryDto } from "@/client/engine-types";
import { call } from "@/client/ipc";
import { engineKeys } from "@/client/queries";
import type { GlobalEntry } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";

/** The default open repo (D2): the first by name, then by `repoId`; `null` without repos. */
export function defaultRepoId(repos: readonly RepoSummaryDto[]): string | null {
  const first = [...repos].sort((a, b) => a.name.localeCompare(b.name) || a.repoId.localeCompare(b.repoId))[0];
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

  useEffect(() => {
    if (repos === undefined) return;
    if (selected !== null && repos.some((r) => r.repoId === selected)) return;
    const next = defaultRepoId(repos);
    if (next !== selected) selectRepo(next);
  }, [repos, selected, selectRepo]);

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
