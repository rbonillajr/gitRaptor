// D7-CACHE (DS-US-DSK-007 T003, D3): useGuardBlocks() takes no argument and rereads `guard_blocks` of the chosen repo on
// mount, when the repo changes, when the window turns visible and every 10 s only while it is visible. No repo, a
// connection that is not live or a restricted session never call. The data comes from the cache, filled by messages.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { engineKeys } from "@/client/queries";
import type { GuardBlocks } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { useGuardBlocks } from "./useGuardBlocks";

let client: QueryClient;
let invoked: { cmd: string; args: unknown }[] = [];
const reads = () => invoked.filter((i) => i.cmd === "guard_blocks").map((i) => (i.args as { params: { repoId: string } }).params.repoId);
const flush = (ms = 0) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const visibility = (state: "visible" | "hidden") => {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
  act(() => void document.dispatchEvent(new Event("visibilitychange")));
};
const LIVE_SESSION = { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true } as const;

function mount() {
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  return renderHook(() => useGuardBlocks(), { wrapper });
}

beforeEach(() => {
  vi.useFakeTimers();
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  invoked = [];
  useEngineStore.setState({ conn: { kind: "live" }, staleSinceMs: null, resync: null, session: { ...LIVE_SESSION } });
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "main" } });
  visibility("visible");
  mockIPC((cmd, args) => {
    invoked.push({ cmd, args });
    return {};
  });
});
afterEach(() => {
  cleanup();
  clearMocks();
  vi.useRealTimers();
});

describe("D7-CACHE useGuardBlocks", () => {
  it("D7-CACHE: reads guard_blocks of the chosen repo when it mounts", async () => {
    mount();
    await flush();
    expect(reads(), "useGuardBlocks must call guard_blocks once on mount").toEqual(["tienda"]);
  });

  it("D7-CACHE: reads every 10 s only while visible", async () => {
    mount();
    await flush();
    await flush(9_999);
    expect(reads(), "nothing before 10 s").toHaveLength(1);
    await flush(1);
    expect(reads(), "one more at 10 s").toHaveLength(2);
    await flush(10_000);
    expect(reads(), "and another at 20 s").toHaveLength(3);
    visibility("hidden");
    await flush(60_000);
    expect(reads(), "a hidden window runs no timer").toHaveLength(3);
    visibility("visible");
    await flush();
    expect(reads(), "turning visible again reads at once").toHaveLength(4);
    await flush(10_000);
    expect(reads(), "and the clock restarts").toHaveLength(5);
  });

  it("D7-CACHE: asks again when the repo changes", async () => {
    mount();
    await flush();
    act(() => useUiStore.getState().selectRepo("notas"));
    await flush();
    expect(reads(), "the new repo is read at once").toEqual(["tienda", "notas"]);
    await flush(10_000);
    expect(reads().slice(2), "and the clock reads the new repo").toEqual(["notas"]);
  });

  it("D7-CACHE: never reads without a repo, without a live connection or in a restricted view", async () => {
    useUiStore.setState({ selectedRepoId: null });
    mount();
    await flush(20_000);
    expect(reads(), "no repo, no read").toEqual([]);
    cleanup();
    useUiStore.setState({ selectedRepoId: "tienda" });
    useEngineStore.setState({ conn: { kind: "connecting" } });
    mount();
    await flush(20_000);
    expect(reads(), "a connection that is not live does not read").toEqual([]);
    cleanup();
    useEngineStore.setState({ conn: { kind: "live" }, session: { ...LIVE_SESSION, restricted: true } });
    mount();
    await flush(20_000);
    expect(reads(), "a restricted session never reads the log").toEqual([]);
  });

  it("D7-CACHE: returns the cache entry of the chosen repo and pending before it arrives", async () => {
    const { result } = mount();
    await flush();
    expect(result.current.isPending, "without data it is pending, never empty").toBe(true);
    expect(result.current.data).toBeUndefined();
    const ready: GuardBlocks = {
      state: "ready",
      repoId: "tienda",
      protection: { state: "full" },
      sinceMs: 0,
      summary: { blocked: 0, exceptions: 0, exceptionsRejected: 0 },
      entries: [],
      pending: null,
      unlogged: [],
    };
    act(() => void client.setQueryData(engineKeys.guard("tienda"), ready));
    await flush();
    expect(result.current.data, "the data is the cache entry of the repo").toEqual(ready);
    expect(reads().length, "the hook must have read the repo").toBeGreaterThan(0);
  });
});
