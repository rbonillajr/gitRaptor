// D8-PANEL (DS-US-DSK-008 D5, D7): EngineCenter mounts the conflict panel above the main view (the fleet today), and
// the pair detail replaces the fleet when the centre has the `pair` destination of the open repo.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { EngineMessage, PairDetailDto, PairDto } from "@/client/engine-types";
import { NOW, pair, tienda } from "@/fleet/fixtures";
import { applyEngineBatch } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { EngineCenter } from "./EngineCenter";

const CONFLICT: PairDto = pair("feat-pagos", "feat-login", {
  pairId: "c1",
  rev: 4,
  state: "current",
  level: "conflict",
  computedAtUtcMs: NOW - 40_000,
  conflictFiles: 1,
});
const DETAIL: PairDetailDto = {
  pair: CONFLICT,
  overlap: [],
  conflict: { files: [{ path: "pagos.rs", kind: "content", ours: [{ start: 40, len: 19 }], theirs: [{ start: 44, len: 18 }], hunksTruncated: false, hasMergeRule: false }], truncated: false },
};
const repoRow = {
  repoId: "tienda",
  name: "tienda",
  path: "/w/tienda/.git",
  state: "observed" as const,
  tier: "active" as const,
  attention: { conflicts: { state: "counted" as const, count: 1 }, denials: { state: "counted" as const, count: 0 } },
};

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  const batch: EngineMessage[] = [
    { type: "global", engine: { state: "observing", gitVersion: "2.45.1" }, daemon: { version: "0.1.0", protocol: 9 }, autostart: "unknown", repos: [repoRow] },
    { type: "repo", fleet: { ...tienda().repo, repoId: "tienda", pairs: [CONFLICT], limits: ["local-only"] } },
    { type: "pairDetails", repoId: "tienda", details: [DETAIL], filled: false, perf: { backendNs: 1 } },
  ];
  applyEngineBatch(client, batch);
  return render(
    <QueryClientProvider client={client}>
      <EngineCenter center={{ kind: "content", stale: false }} conn={{ kind: "live" }} gitVersion="2.45.1" candidates={[]} onRetry={() => {}} />
    </QueryClientProvider>,
  );
}

beforeEach(async () => {
  mockIPC(() => ({}));
  useEngineStore.setState({
    conn: { kind: "live" },
    staleSinceMs: null,
    resync: null,
    session: { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true },
  });
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "main" } });
  await act(async () => void (await i18n.changeLanguage("en")));
});
afterEach(() => {
  cleanup();
  clearMocks();
});

describe("EngineCenter conflicts", () => {
  it("the conflict panel sits above the main view and not inside the fleet", () => {
    mount();
    const panel = screen.getByRole("region", { name: "Predicted conflicts" });
    const fleet = screen.getByRole("list", { name: "Worktrees of tienda" });
    expect(panel.compareDocumentPosition(fleet) & Node.DOCUMENT_POSITION_FOLLOWING, "the panel comes before the fleet").toBeTruthy();
    expect(panel.contains(fleet), "the fleet is not inside the panel").toBe(false);
    expect(fleet.contains(panel), "the panel is not inside the fleet list").toBe(false);
    expect(within(panel).getByText("pagos.rs"), "the panel paints the file of the conflict").toBeInTheDocument();
  });

  it("the pair destination replaces the main view with the detail of the pair", () => {
    useUiStore.setState({ center: { view: "pair", repoId: "tienda", pairId: "c1" } });
    mount();
    expect(screen.getByRole("heading", { level: 2, name: /are going to collide/ }), "the detail of the pair").toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Worktrees of tienda" }), "the fleet gives way to the detail").toBeNull();
    expect(screen.getByRole("button", { name: "Back to the fleet" })).toBeInTheDocument();
  });
});
