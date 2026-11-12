// DS-US-DSK-005 T011 (§ 9.4, TS column): the conflict band in the graph through the center: the badge in the Branch cell
// of both lanes, what each lane says in its accessible name, and the panel of DS-US-DSK-008 above the graph.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { EngineMessage, PairDto } from "@/client/engine-types";
import { EngineCenter } from "@/engine/EngineCenter";
import { applyEngineBatch } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { NOW, pair, tienda, tiendaGraph } from "./fixtures";

const CONFLICT: PairDto = pair("feat-pagos", "feat-login", { pairId: "c1", computedAtUtcMs: NOW - 40_000, conflictFiles: 1 });
const repoRow = {
  repoId: "tienda",
  name: "tienda",
  path: "/w/tienda/.git",
  state: "observed" as const,
  tier: "active" as const,
  attention: { conflicts: { state: "counted" as const, count: 1 }, denials: { state: "counted" as const, count: 0 } },
};

function mount(pairs: PairDto[]) {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  const batch: EngineMessage[] = [
    { type: "global", engine: { state: "observing", gitVersion: "2.45.1" }, daemon: { version: "0.1.0", protocol: 9 }, autostart: "unknown", repos: [repoRow] },
    { type: "repo", fleet: { ...tienda().repo, repoId: "tienda", pairs } },
    { type: "graph", repoId: "tienda", graph: tiendaGraph(), perf: { backendNs: 1 } },
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
  useEngineStore.setState({ conn: { kind: "live" }, staleSinceMs: null, resync: null, session: { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true } });
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "main" } });
  await act(async () => void (await i18n.changeLanguage("en")));
});
afterEach(() => {
  cleanup();
  clearMocks();
});

const headOf = (folder: string) => screen.getByRole("row", { name: new RegExp(`^${folder}, commit`) });

describe("conflict band in the graph", () => {
  it("both lanes of a conflict show the badge and say it in their name", () => {
    mount([CONFLICT]);
    for (const folder of ["feat-pagos", "feat-login"]) {
      const head = headOf(folder);
      const badge = within(head).getByRole("group", { name: /predicted conflict/i });
      expect(badge).toHaveTextContent("⚡");
      expect(head, "VoiceOver reads the conflict in the name of the lane").toHaveAccessibleName(expect.stringMatching(/predicted conflict/i));
    }
    const rows = screen.getAllByRole("row").filter((r) => r.hasAttribute("data-row-id"));
    const withBadge = rows.filter((r) => within(r).queryByRole("group", { name: /predicted conflict/i }) !== null);
    expect(withBadge, "one badge per lane head, none on the other rows").toHaveLength(2);
  });

  it("a lane with no conflict has no badge and no extra words in its name", () => {
    mount([]);
    expect(within(headOf("feat-pagos")).queryByRole("group")).toBeNull();
    expect(headOf("feat-pagos").getAttribute("aria-label")).not.toMatch(/conflict/i);
  });

  it("the conflict panel stays above the graph", () => {
    mount([CONFLICT]);
    const panel = screen.getByRole("region", { name: "Predicted conflicts" });
    const grid = screen.getByRole("grid", { name: "Lane graph of tienda" });
    expect(panel.compareDocumentPosition(grid) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(panel.contains(grid)).toBe(false);
  });

  it("the canvas stays hidden and the text equivalent is the panel and the lane name", () => {
    const { container } = mount([CONFLICT]);
    const canvas = container.getElementsByTagName("canvas")[0];
    expect(canvas).toHaveAttribute("aria-hidden", "true");
  });
});
