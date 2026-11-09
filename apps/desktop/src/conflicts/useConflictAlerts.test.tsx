// D8-ALERTS (DS-US-DSK-008 D12-D15, D17, DS-US-CKP-008 E1-E5): the in-app notice of a new predicted conflict. The hook
// watches the open repo and the global scope and speaks through the app's toast (kind info, role status); there is
// no OS notification. Fake timers drive the 2 s wait for the first file; `flush` lets the query cache notify.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { EngineMessage, PairDetailDto, PairDto } from "@/client/engine-types";
import { NOW, pair, tienda } from "@/fleet/fixtures";
import { applyEngineBatch } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { ToastProvider } from "@/ui";
import { useConflictAlerts } from "./useConflictAlerts";

const A = "Claude Code · feat-pagos";
const B = "Claude Code · feat-login";
const perf = { backendNs: 1 };
const row = (repoId: string, count: number) => ({
  repoId,
  name: repoId,
  path: `/w/${repoId}/.git`,
  state: "observed" as const,
  tier: "active" as const,
  attention: { conflicts: { state: "counted" as const, count }, denials: { state: "counted" as const, count: 0 } },
});
const global = (counts: Record<string, number>): EngineMessage => ({
  type: "global",
  engine: { state: "observing", gitVersion: "2.45.1" },
  daemon: { version: "0.1.0", protocol: 9 },
  autostart: "unknown",
  repos: Object.entries(counts).map(([id, n]) => row(id, n)),
});
const bolt = (id: string, a = "feat-pagos", b = "feat-login", over: Partial<PairDto> = {}): PairDto =>
  pair(a, b, { pairId: id, rev: 5, state: "current", level: "conflict", computedAtUtcMs: NOW - 1_000, conflictFiles: 1, ...over });
const detailOf = (p: PairDto, path = "pagos.rs"): PairDetailDto => ({
  pair: p,
  overlap: [],
  conflict: { files: [{ path, kind: "content", ours: [{ start: 1, len: 2 }], theirs: [{ start: 3, len: 4 }], hunksTruncated: false, hasMergeRule: false }], truncated: false },
});
const repo = (pairs: PairDto[], id = "tienda"): EngineMessage => ({ type: "repo", fleet: { ...tienda().repo, repoId: id, pairs } });
const prediction = (pairs: PairDto[], removed: string[] = []): EngineMessage => ({ type: "prediction", repoId: "tienda", pairs, removed, perf });
const details = (...d: PairDetailDto[]): EngineMessage => ({ type: "pairDetails", repoId: "tienda", details: d, filled: false, perf });

let client: QueryClient;
const flush = (ms = 0) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const apply = async (...batch: EngineMessage[]) => {
  act(() => applyEngineBatch(client, batch));
  await flush();
};
const toasts = () => screen.queryAllByRole("status").filter((n) => n.hasAttribute("data-toast"));

function Probe() {
  useConflictAlerts();
  return null;
}
function mount() {
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <Probe />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "main" } });
  await act(async () => void (await i18n.changeLanguage("en")));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useConflictAlerts", () => {
  it("a new conflict shows a toast with the pair of agents", async () => {
    mount();
    await apply(global({ tienda: 0, notas: 0 }), repo([]));
    expect(toasts(), "an empty repo says nothing").toHaveLength(0);
    const p = bolt("c1");
    await apply(prediction([p]), details(detailOf(p)));
    await flush(10);
    expect(toasts(), "one toast for the new conflict").toHaveLength(1);
    expect(toasts()[0]?.textContent, "info: the glyph is in the message, with the pair and the first file").toContain(`⚡ Predicted conflict: ${A} ↔ ${B} in pagos.rs`);
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    expect(useUiStore.getState().center, "View opens the detail of the pair").toEqual({ view: "pair", repoId: "tienda", pairId: "c1" });
  });

  it("conflicts present when the repo opens do not toast", async () => {
    mount();
    await apply(global({ tienda: 1 }), repo([bolt("c1")]));
    await flush(3_000);
    expect(toasts(), "E1: the conflicts already there are seeded").toHaveLength(0);
    const p = bolt("c2", "feat-login", "base");
    await apply(prediction([p]), details(detailOf(p, "README.md")));
    await flush(10);
    expect(toasts(), "a conflict that appears afterwards does").toHaveLength(1);
  });

  it("waits up to 2 s for the file", async () => {
    mount();
    await apply(global({ tienda: 0 }), repo([]));
    const p = bolt("c1");
    await apply(prediction([p]));
    await flush(1_999);
    expect(toasts(), "E3: the toast waits for the first file").toHaveLength(0);
    await flush(1);
    expect(toasts(), "at 2 s it goes out without the file").toHaveLength(1);
    expect(toasts()[0]?.textContent).toContain(`⚡ Predicted conflict: ${A} ↔ ${B}`);
    expect(toasts()[0]?.textContent).not.toContain("pagos.rs");
  });

  it("the file that arrives before 2 s goes in the toast", async () => {
    mount();
    await apply(global({ tienda: 0 }), repo([]));
    const p = bolt("c1");
    await apply(prediction([p]));
    await flush(1_000);
    expect(toasts()).toHaveLength(0);
    await apply(details(detailOf(p)));
    await flush(10);
    expect(toasts(), "one toast, with the file").toHaveLength(1);
    expect(toasts()[0]?.textContent).toContain("in pagos.rs");
    await flush(3_000);
    expect(toasts(), "and no second one when the timer would have fired").toHaveLength(1);
  });

  it("a toast of a conflict that is gone is dismissed", async () => {
    mount();
    await apply(global({ tienda: 0 }), repo([]));
    const p = bolt("c1");
    await apply(prediction([p]), details(detailOf(p)));
    await flush(10);
    expect(toasts()).toHaveLength(1);
    await apply(prediction([{ ...p, rev: 6, level: "overlap" }]));
    await flush(10);
    expect(toasts(), "E4: the pair stopped being ⚡ before the toast expired").toHaveLength(0);
    const q = bolt("c2", "feat-login", "base");
    await apply(prediction([q]), details(detailOf(q)));
    await flush(10);
    expect(toasts()).toHaveLength(1);
    await apply(prediction([], ["c2"]));
    await flush(10);
    expect(toasts(), "a removed pair retires its toast too").toHaveLength(0);
  });

  it("a rising count in another repo shows a toast without pair names", async () => {
    mount();
    await apply(global({ tienda: 0, notas: 0 }), repo([]));
    expect(toasts(), "the first global snapshot seeds the counts").toHaveLength(0);
    await apply(global({ tienda: 0, notas: 1 }));
    await flush(10);
    expect(toasts(), "D15: one toast for the other repo").toHaveLength(1);
    expect(toasts()[0]?.textContent).toContain("⚡ New predicted conflict in notas");
    expect(toasts()[0]?.textContent, "the global scope carries no pair names").not.toContain("Claude Code");
    fireEvent.click(screen.getByRole("button", { name: "View" }));
    expect(useUiStore.getState().selectedRepoId, "View opens that repo").toBe("notas");
  });

  it("three or more new conflicts in one pass show a single toast", async () => {
    mount();
    await apply(global({ tienda: 0 }), repo([]));
    const pairs = [bolt("c1"), bolt("c2", "feat-pagos", "base"), bolt("c3", "feat-login", "base")];
    await apply(prediction(pairs), details(...pairs.map((p) => detailOf(p))));
    await flush(10);
    expect(toasts(), "D17: one toast per wave").toHaveLength(1);
    expect(toasts()[0]?.textContent).toContain("⚡ 3 new predicted conflicts");
    await flush(3_000);
    expect(toasts().filter((n) => n.textContent?.includes("Predicted conflict:")), "no toast per pair").toHaveLength(0);
  });

  it("two new conflicts in one pass show one toast each", async () => {
    mount();
    await apply(global({ tienda: 0 }), repo([]));
    const pairs = [bolt("c1"), bolt("c2", "feat-pagos", "base")];
    await apply(prediction(pairs), details(...pairs.map((p) => detailOf(p))));
    await flush(10);
    expect(toasts(), "1 or 2 ids: one toast per pair").toHaveLength(2);
  });
});
