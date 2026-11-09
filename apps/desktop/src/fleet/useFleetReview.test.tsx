// Review M1: a repo the backend closes must not be reopened in a loop; the tree goes back to the previous repo.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { EngineMessage, RepoSummaryDto } from "@/client/engine-types";
import { applyEngineBatch } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { useFleet } from "./useFleet";

const summary = (repoId: string, name: string, state: RepoSummaryDto["state"] = "observed"): RepoSummaryDto => ({
  repoId,
  name,
  path: `/w/${name}/.git`,
  state,
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
let invoked: string[] = [];
const apply = (...batch: EngineMessage[]) => act(() => applyEngineBatch(client, batch));
const selected = () => useUiStore.getState().selectedRepoId;

beforeEach(() => {
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  invoked = [];
  useUiStore.setState({ selectedRepoId: null, previousRepoId: null });
  useEngineStore.setState({ closedRepoIds: new Set() });
  mockIPC((cmd, args) => {
    if (cmd === "fleet_open") invoked.push((args as { params: { repoId: string } }).params.repoId);
    return {};
  });
  renderHook(() => useFleet(), {
    wrapper: ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>,
  });
});
afterEach(() => {
  cleanup();
  clearMocks();
});

describe("M1 repoClosed", () => {
  it("repoClosed of the chosen repo does not open the same repo again", async () => {
    await apply(global(summary("r1", "alpha")));
    await waitFor(() => expect(invoked).toEqual(["r1"]));
    await apply({ type: "repoClosed", repoId: "r1" });
    await new Promise((r) => setTimeout(r, 50));
    expect(invoked, "no reopening loop").toEqual(["r1"]);
    expect(selected()).toBeNull();
    await apply(global(summary("r1", "alpha")));
    await waitFor(() => expect(invoked, "a new global lifts the exclusion").toEqual(["r1", "r1"]));
  });

  it("repoClosed goes back to the previous repo, and skips unavailable ones by default", async () => {
    await apply(global(summary("r1", "alpha"), summary("r2", "beta"), summary("r0", "aaa", "unavailable")));
    await waitFor(() => expect(selected(), "the unavailable repo is never the default").toBe("r1"));
    act(() => useUiStore.getState().selectRepo("r2"));
    await apply({ type: "repoClosed", repoId: "r2" });
    await waitFor(() => expect(selected(), "back to the previous repo").toBe("r1"));
  });
});
