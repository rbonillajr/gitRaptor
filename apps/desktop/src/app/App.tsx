import { QueryClientProvider } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { watchEngine } from "@/client/engine-stream";
import { engineKeys } from "@/client/queries";
import { engineView } from "@/engine/engine-view";
import { defaultRepoId } from "@/fleet/useFleet";
import "@/i18n";
import { AppShell } from "@/shell/AppShell";
import { applyEngineBatch, type GlobalEntry } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { queryClient } from "@/state/query-client";
import { useUiStore } from "@/state/ui-store";
import { ToastProvider } from "@/ui";

const REPO_TYPES: ReadonlySet<string> = new Set(["repo", "worktrees", "sessions", "repoSession", "prediction"]);

/** Whether the engine state is one the user can read as the answer (NFR-13.6), not a "still working". */
function isFinal(): boolean {
  const { conn, staleSinceMs, resync, session } = useEngineStore.getState();
  const global = queryClient.getQueryData<GlobalEntry>(engineKeys.global);
  const { center } = engineView({ conn, staleSinceMs, resync, session, global, openRepoTier: null });
  if (center.kind === "skeleton") return false;
  // With a repo chosen, the useful paint is the fleet, not its skeleton.
  const target = useUiStore.getState().selectedRepoId ?? defaultRepoId(global?.repos ?? [], useEngineStore.getState().closedRepoIds);
  if (center.kind === "content" && target !== null && queryClient.getQueryData(engineKeys.repo(target)) === undefined) return false;
  return center.kind !== "content" || conn.kind === "live";
}

/**
 * Root of the webview: the query cache around the B1 window. The i18n is initialised on import. It opens the
 * engine stream once and feeds each frame's batch to the store and the cache.
 */
export function App() {
  // A ref survives StrictMode's second effect run, so the stream opens once per load.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    let marked = false;
    void watchEngine((batch, { receivedAt }) => {
      applyEngineBatch(queryClient, batch);
      const n = batch.filter((m) => REPO_TYPES.has(m.type)).length;
      if (n > 0) useEngineStore.getState().noteRepoBatch({ receivedAt, n });
      if (!marked && isFinal()) {
        marked = true;
        // One frame later: the commit of this batch has painted.
        requestAnimationFrame(() => performance.mark("gr:first-meaningful-paint"));
      }
    });
  }, []);

  return (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <AppShell />
      </ToastProvider>
    </QueryClientProvider>
  );
}
