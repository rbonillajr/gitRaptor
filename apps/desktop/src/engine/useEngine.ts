import { skipToken, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { CandidateDto, ConnState } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import type { GlobalEntry } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { type EngineView, engineView } from "./engine-view";

export interface EngineSnapshot {
  view: EngineView;
  conn: ConnState;
  gitVersion: string | null;
  candidates: readonly CandidateDto[];
}

const NO_CANDIDATES: readonly CandidateDto[] = [];

/**
 * The engine as the window paints it: the connection from the store and the data from the query cache, folded by
 * `engineView`. Read-only: the cache entries have no `queryFn` and are written only by `applyEngineBatch`.
 */
export function useEngine(): EngineSnapshot {
  const { i18n } = useTranslation();
  const conn = useEngineStore((s) => s.conn);
  const staleSinceMs = useEngineStore((s) => s.staleSinceMs);
  const resync = useEngineStore((s) => s.resync);
  const session = useEngineStore((s) => s.session);
  const global = useQuery<GlobalEntry>({ queryKey: engineKeys.global, queryFn: skipToken }).data;
  const discovered = useQuery<CandidateDto[]>({ queryKey: engineKeys.discovered, queryFn: skipToken }).data;

  const view = engineView({ conn, staleSinceMs, resync, session, global, openRepoTier: null, locale: i18n.language });
  return { view, conn, gitVersion: global?.engine.gitVersion ?? null, candidates: discovered ?? NO_CANDIDATES };
}
