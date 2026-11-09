import { skipToken, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import type { GlobalEntry } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { fleetCards } from "./fleet-model";
import { RepoTree } from "./RepoTree";
import { useNow } from "./useNow";

/** The sidebar tree wired to the cache and the store: repos from `global`, the open repo's cards in fleet order. */
export function RepoTreeContainer() {
  const { t } = useTranslation();
  const selected = useUiStore((s) => s.selectedRepoId);
  const selectRepo = useUiStore((s) => s.selectRepo);
  const activity = useEngineStore((s) => s.session?.activity ?? false);
  const global = useQuery<GlobalEntry>({ queryKey: engineKeys.global, queryFn: skipToken }).data;
  const fleet = useQuery<RepoFleet>({ queryKey: engineKeys.repo(selected ?? ""), queryFn: skipToken, enabled: selected !== null }).data;
  const nowMs = useNow();
  const cards = useMemo(() => (fleet === undefined ? [] : fleetCards(fleet, nowMs, activity, t)), [fleet, nowMs, activity, t]);
  return <RepoTree repos={global?.repos} selectedRepoId={selected} cards={cards} onOpen={selectRepo} />;
}
