import { skipToken, useQuery, type UseQueryResult } from "@tanstack/react-query";
import { useEffect } from "react";
import { call } from "@/client/ipc";
import { engineKeys } from "@/client/queries";
import type { GuardBlocks } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";

const REREAD_MS = 10_000;

/**
 * Mounted once by `EngineCenter`: asks the backend to read `guard_blocks` of the chosen repo on mount, when the repo
 * changes, when the window turns visible and every 10 s only while it is visible (D3: the engine publishes no event
 * for a denial, NFR-13.8: a hidden window runs no timer). It never reads with no repo, a connection that is not live
 * or a restricted session. The data is the cache entry, which only `applyEngineBatch` writes.
 */
export function useGuardBlocks(): Pick<UseQueryResult<GuardBlocks>, "data" | "isPending"> {
  const selected = useUiStore((s) => s.selectedRepoId);
  const canRead = useEngineStore((s) => s.conn.kind === "live" && s.session !== null && !s.session.restricted);

  useEffect(() => {
    if (selected === null || !canRead) return;
    let timer: ReturnType<typeof setInterval> | null = null;
    const read = () => void call("guard_blocks", { repoId: selected }).catch(() => undefined);
    const stop = () => {
      if (timer !== null) clearInterval(timer);
      timer = null;
    };
    const sync = () => {
      stop();
      if (document.visibilityState !== "visible") return;
      read();
      timer = setInterval(read, REREAD_MS);
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => {
      document.removeEventListener("visibilitychange", sync);
      stop();
    };
  }, [selected, canRead]);

  const { data, isPending } = useQuery<GuardBlocks>({
    queryKey: engineKeys.guard(selected ?? ""),
    queryFn: skipToken,
    enabled: selected !== null,
  });
  return { data, isPending };
}
