// D4-USEFLEET (DS-US-DSK-004 T003): useFleet picks the open repo by default (first by name, then repoId) when the
// global snapshot arrives with none chosen or the chosen one disappears, calls fleet_open on every change of
// selectedRepoId, and reads the repo entry from the cache with no network queryFn.
//
// `useFleet()` takes no argument and returns the UseQueryResult of engineKeys.repo(selectedRepoId): `data` is the `RepoFleet`.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { EngineMessage, RepoFleet, RepoSummaryDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { applyEngineBatch } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { tienda } from "./fixtures";
import { useFleet } from "./useFleet";

const summary = (repoId: string, name: string): RepoSummaryDto => ({
  repoId,
  name,
  path: `/w/${name}/.git`,
  state: "observed",
  tier: "active",
  attention: { conflicts: { state: "counted", count: 0 }, denials: { state: "counted", count: 0 } },
});
const global = (...repos: RepoSummaryDto[]): EngineMessage => ({
  type: "global",
  engine: { state: "observing", gitVersion: "2.45.1" },
  daemon: { version: "0.1.0", protocol: 9 },
  autostart: "unknown",
  repos,
});

let client: QueryClient;
let invoked: { cmd: string; args: unknown }[] = [];
const opens = () => invoked.filter((i) => i.cmd === "fleet_open").map((i) => (i.args as { params: { repoId: string } }).params.repoId);
const apply = (...batch: EngineMessage[]) => act(() => applyEngineBatch(client, batch));
const selected = () => useUiStore.getState().selectedRepoId;

function mount() {
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  return renderHook(() => useFleet() as unknown as { data?: RepoFleet }, { wrapper });
}
const fleetOf = (r: { current: { data?: RepoFleet } | undefined }) => r.current?.data;

beforeEach(() => {
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  invoked = [];
  useUiStore.setState({ selectedRepoId: null });
  mockIPC((cmd, args) => {
    invoked.push({ cmd, args });
    return {};
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("D4-USEFLEET useFleet", () => {
  it("useFleet: picks the first repo by name and then by repoId when the global snapshot arrives with none chosen", async () => {
    mount();
    expect(selected(), "nothing is chosen before the snapshot").toBeNull();
    expect(opens(), "fleet_open is not called without a repo").toEqual([]);
    await apply(global(summary("r9", "zeta"), summary("r3", "alpha"), summary("r1", "alpha"), summary("r2", "beta")));
    await waitFor(() => expect(selected(), "the first by name, then by repoId").toBe("r1"));
    await waitFor(() => expect(opens()).toEqual(["r1"]));
  });

  it("useFleet: keeps the chosen repo when it is in the snapshot", async () => {
    useUiStore.getState().selectRepo("r3");
    mount();
    await apply(global(summary("r1", "alpha"), summary("r3", "beta")));
    await waitFor(() => expect(opens()).toEqual(["r3"]));
    expect(selected()).toBe("r3");
  });

  it("useFleet: calls fleet_open with { params: { repoId } } on every change of selectedRepoId, once each", async () => {
    mount();
    await apply(global(summary("r1", "alpha"), summary("r2", "beta")));
    await waitFor(() => expect(opens()).toEqual(["r1"]));
    expect(invoked.find((i) => i.cmd === "fleet_open")?.args, "the params envelope").toEqual({ params: { repoId: "r1" } });
    act(() => useUiStore.getState().selectRepo("r2"));
    await waitFor(() => expect(opens()).toEqual(["r1", "r2"]));
    await apply({ type: "repoTier", repoId: "r2", tier: "dormant" });
    act(() => useUiStore.getState().selectRepo("r2"));
    expect(opens(), "an unchanged selection does not open again").toEqual(["r1", "r2"]);
  });

  it("useFleet: picks another repo when the chosen one disappears", async () => {
    mount();
    await apply(global(summary("r1", "alpha"), summary("r2", "beta"), summary("r3", "gamma")));
    await waitFor(() => expect(selected()).toBe("r1"));
    await apply({ type: "repoRetired", repoId: "r1" });
    await waitFor(() => expect(selected(), "the first of the rest").toBe("r2"));
    await waitFor(() => expect(opens()).toEqual(["r1", "r2"]));
  });

  it("useFleet: reads the repo entry from the cache with no network queryFn", async () => {
    const { result } = mount();
    await apply(global(summary("tienda", "tienda")));
    await waitFor(() => expect(selected()).toBe("tienda"));
    expect(fleetOf(result), "no entry until the repo snapshot is in").toBeUndefined();
    const fleet = tienda();
    await apply({ type: "repo", fleet: fleet.repo });
    await waitFor(() => expect(fleetOf(result)).toEqual({ repo: fleet.repo, sessions: new Map(), detection: null }));
    await apply({ type: "sessions", repoId: "tienda", detectionAvailable: true, sessions: [...fleet.sessions.values()] });
    await waitFor(() => expect(fleetOf(result)?.sessions.size, "a later message reaches the hook").toBe(2));
    const query = client.getQueryCache().find({ queryKey: engineKeys.repo("tienda") });
    expect(typeof query?.options.queryFn, "the entry has no function that could go to the network").not.toBe("function");
    expect(
      invoked.map((i) => i.cmd).filter((cmd) => cmd !== "fleet_open"),
      "nothing but fleet_open goes to the backend",
    ).toEqual([]);
  });
});
