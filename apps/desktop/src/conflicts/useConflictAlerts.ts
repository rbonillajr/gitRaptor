import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { PairDto, RepoFleet } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { fleetCards } from "@/fleet/fleet-model";
import type { GlobalEntry } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { useToast } from "@/ui";
import { nextAlerts } from "./alerts";
import { focusConflictPanel } from "./focus";
import { sideName } from "./prediction-model";

/** How long a new conflict waits for its first file before the toast goes out without it (D13, E3). */
const FILE_WAIT_MS = 2000;
/** From this many new conflicts in one pass a single toast speaks for all (D17). */
const WAVE = 3;

type Latest = RefObject<{ toast: ReturnType<typeof useToast>; t: ReturnType<typeof useTranslation>["t"] }>;

/**
 * The in-app notice of a new predicted conflict (DS-US-DSK-008 D12-D15, D17): a toast of kind `info` for the open repo
 * (with the pair and its first file) and for a rising count in another repo. No OS notification (D14). Mount it once,
 * under the `ToastProvider`. It listens to the query cache, which only `applyEngineBatch` writes.
 */
export function useConflictAlerts(): void {
  const client = useQueryClient();
  const toast = useToast();
  const { t } = useTranslation();
  const latest = useRef({ toast, t });
  useEffect(() => {
    latest.current = { toast, t };
  });
  useEffect(() => startAlerts(client, latest), [client]);
}

function startAlerts(client: QueryClient, latest: Latest): () => void {
  useEngineStore.setState({ alertState: new Map(), seededCounts: null });
  const seen = new Set<string>();
  const lastFleet = new Map<string, RepoFleet>();
  let lastGlobal: GlobalEntry | undefined;
  const waiting = new Map<string, { repoId: string; timer: ReturnType<typeof setTimeout> }>();
  const toasts = new Map<string, { repoId: string; id: string }>();
  let wave: { id: string; repoId: string; pairIds: string[] } | null = null;

  const readFleet = (repoId: string) => client.getQueryData<RepoFleet>(engineKeys.repo(repoId));
  const known = (repoId: string) => useEngineStore.getState().alertState.get(repoId);
  const openPair = (repoId: string, pairId: string) => {
    const ui = useUiStore.getState();
    if (ui.selectedRepoId !== repoId) ui.selectRepo(repoId);
    useUiStore.getState().openPairDetail({ repoId, pairId });
  };

  function emit(repoId: string, p: PairDto) {
    const fleet = readFleet(repoId);
    if (fleet === undefined) return;
    const { toast, t } = latest.current;
    const cards = fleetCards(fleet, Date.now(), true, t);
    // The toast can outrun `sessions.list`: with no session known the MVP agent (Claude Code, D2) and the folder name the side.
    const label = (key: string) => {
      const folder = fleet.repo.worktrees.find((w) => w.key === key)?.folder;
      return key === "base" || cards.find((c) => c.key === key)?.agent.sessionId != null || folder === undefined
        ? sideName(fleet, cards, key, t)
        : t("fleet.agent.inWorktree", { agent: t("fleet.agent.claudeCode"), worktree: folder });
    };
    const a = label(p.a);
    const b = label(p.b);
    const detail = fleet.details.get(p.pairId);
    const file = detail !== undefined && detail.pair.rev === p.rev ? detail.conflict?.files[0]?.path : undefined;
    const extra = p.conflictFiles - 1;
    const message =
      file === undefined
        ? t("conflicts.toast.noFile", { a, b })
        : extra > 0
          ? t("conflicts.toast.withFileMore", { a, b, file, extra })
          : t("conflicts.toast.withFile", { a, b, file, count: 1 });
    const id = toast.show({ kind: "info", message, action: { label: t("conflicts.toast.open"), onAction: () => openPair(repoId, p.pairId) } });
    toasts.set(p.pairId, { repoId, id });
  }

  function raise(repoId: string, ids: readonly string[], fleet: RepoFleet) {
    const { toast, t } = latest.current;
    if (ids.length >= WAVE) {
      const id = toast.show({
        kind: "info",
        message: t("conflicts.toast.many", { count: ids.length }),
        action: { label: t("conflicts.toast.open"), onAction: focusConflictPanel },
      });
      wave = { id, repoId, pairIds: [...ids] };
      return;
    }
    for (const pairId of ids) {
      const p = fleet.repo.pairs.find((x) => x.pairId === pairId);
      if (p === undefined) continue;
      if (fleet.details.get(pairId)?.pair.rev === p.rev) emit(repoId, p);
      else waiting.set(pairId, { repoId, timer: setTimeout(() => settle(pairId, true), FILE_WAIT_MS) });
    }
  }

  /** A pair that waited for its file goes out with it, or (when the wait is over) without it. */
  function settle(pairId: string, force: boolean) {
    const w = waiting.get(pairId);
    if (w === undefined) return;
    const fleet = readFleet(w.repoId);
    const p = fleet?.repo.pairs.find((x) => x.pairId === pairId);
    if (fleet === undefined || p === undefined) return;
    if (!force && fleet.details.get(pairId)?.pair.rev !== p.rev) return;
    clearTimeout(w.timer);
    waiting.delete(pairId);
    if (known(w.repoId)?.has(pairId)) emit(w.repoId, p);
  }

  /** E4: what stopped being a conflict retires its toast and its wait. */
  function retire() {
    const { toast } = latest.current;
    for (const [pairId, w] of waiting) {
      if (!known(w.repoId)?.has(pairId)) {
        clearTimeout(w.timer);
        waiting.delete(pairId);
      }
    }
    for (const [pairId, entry] of toasts) {
      if (!known(entry.repoId)?.has(pairId)) {
        toast.dismiss(entry.id);
        toasts.delete(pairId);
      }
    }
    if (wave !== null) {
      const w = wave;
      if (!w.pairIds.some((id) => known(w.repoId)?.has(id))) {
        toast.dismiss(w.id);
        wave = null;
      }
    }
  }

  /** D15: a rising counted `attention.conflicts` of a repo that is not open; the first global snapshot seeds. */
  function otherRepos() {
    const global = client.getQueryData<GlobalEntry>(engineKeys.global);
    if (global === undefined || global === lastGlobal) return;
    lastGlobal = global;
    const counts = new Map<string, number>();
    for (const r of global.repos) if (r.attention.conflicts.state === "counted") counts.set(r.repoId, r.attention.conflicts.count);
    const previous = useEngineStore.getState().seededCounts;
    useEngineStore.getState().setSeededCounts(counts);
    if (previous === null) return;
    const open = useUiStore.getState().selectedRepoId;
    const { toast, t } = latest.current;
    for (const r of global.repos) {
      const now = counts.get(r.repoId);
      if (r.repoId === open || now === undefined || now <= (previous.get(r.repoId) ?? 0)) continue;
      toast.show({
        kind: "info",
        message: t("conflicts.toast.otherRepo", { repo: r.name }),
        action: { label: t("conflicts.toast.open"), onAction: () => useUiStore.getState().selectRepo(r.repoId) },
      });
    }
  }

  function process() {
    otherRepos();
    for (const repoId of [...seen]) {
      if (readFleet(repoId) === undefined) {
        seen.delete(repoId);
        lastFleet.delete(repoId);
        useEngineStore.getState().setRepoAlerts(repoId, null);
      }
    }
    const repoId = useUiStore.getState().selectedRepoId;
    const fleet = repoId === null ? undefined : readFleet(repoId);
    if (repoId !== null && fleet !== undefined && lastFleet.get(repoId) !== fleet) {
      lastFleet.set(repoId, fleet);
      const first = !seen.has(repoId);
      seen.add(repoId);
      const { state, raised } = nextAlerts(known(repoId) ?? new Map(), {
        kind: "snapshot",
        pairs: fleet.repo.pairs,
        truncated: fleet.repo.pairsTruncated,
        first,
      });
      useEngineStore.getState().setRepoAlerts(repoId, state);
      if (raised.length > 0) raise(repoId, raised, fleet);
    }
    for (const pairId of [...waiting.keys()]) settle(pairId, false);
    retire();
  }

  const unsubscribeCache = client.getQueryCache().subscribe(process);
  const unsubscribeUi = useUiStore.subscribe(process);
  process();
  return () => {
    unsubscribeCache();
    unsubscribeUi();
    for (const w of waiting.values()) clearTimeout(w.timer);
  };
}
