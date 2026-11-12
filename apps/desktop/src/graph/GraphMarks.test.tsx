// DS-US-DSK-005 T009 and T006 step 6 (§ 9.4): the User Timing marks of the graph: the first paint once per repo, and each
// graph delta measured once from the moment its batch was received.
import { act, cleanup, render, waitFor } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { useEngineStore } from "@/state/engine-store";
import { GraphView } from "./GraphView";
import { entryOf, modelOf, tienda, tiendaGraph } from "./fixtures";

const model = () => modelOf(tienda(), entryOf(tiendaGraph()));
const names = (spy: { mock: { calls: unknown[][] } }) => spy.mock.calls.map((c) => c[0]);

let mark: ReturnType<typeof vi.spyOn>;
let measure: ReturnType<typeof vi.spyOn>;

beforeEach(async () => {
  mark = vi.spyOn(performance, "mark").mockImplementation(() => ({}) as PerformanceMark);
  measure = vi.spyOn(performance, "measure").mockImplementation(() => ({}) as PerformanceMeasure);
  useEngineStore.setState({ lastGraphBatch: null });
  await act(async () => void (await i18n.changeLanguage("en")));
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const batch = (receivedAt: number) => act(() => useEngineStore.getState().noteGraphBatch({ receivedAt, n: 1 }));

describe("graph marks", () => {
  it("measures each graph delta once", async () => {
    const { rerender } = render(<GraphView model={model()} repoName="tienda" stale={false} />);
    batch(100);
    await waitFor(() => expect(measure).toHaveBeenCalledWith("gr:graph-delta", { start: 100 }));
    rerender(<GraphView model={model()} repoName="tienda" stale={false} />);
    await act(async () => void (await new Promise((r) => setTimeout(r, 40))));
    expect(names(measure).filter((n) => n === "gr:graph-delta"), "a re-render does not measure again").toHaveLength(1);
    batch(250);
    await waitFor(() => expect(measure).toHaveBeenCalledWith("gr:graph-delta", { start: 250 }));
    expect(names(measure).filter((n) => n === "gr:graph-delta")).toHaveLength(2);
  });

  it("does not measure a batch that was already in the store when the view mounted", async () => {
    useEngineStore.setState({ lastGraphBatch: { receivedAt: 5, n: 1 } });
    render(<GraphView model={model()} repoName="tienda" stale={false} />);
    await act(async () => void (await new Promise((r) => setTimeout(r, 40))));
    expect(measure).not.toHaveBeenCalled();
  });

  it("marks the first graph paint once per repo", async () => {
    const first = render(<GraphView model={model()} repoName="tienda" stale={false} />);
    batch(100);
    batch(200);
    await waitFor(() => expect(names(measure).filter((n) => n === "gr:graph-delta").length).toBeGreaterThan(0));
    await act(async () => void (await new Promise((r) => setTimeout(r, 40))));
    expect(names(mark).filter((n) => n === "gr:graph-first-paint"), "only the first batch of the repo").toHaveLength(1);
    batch(300);
    await act(async () => void (await new Promise((r) => setTimeout(r, 40))));
    expect(names(mark).filter((n) => n === "gr:graph-first-paint")).toHaveLength(1);
    first.unmount();

    // Another repo opens its own view: its first graph is marked again.
    render(<GraphView model={model()} repoName="notas" stale={false} />);
    batch(400);
    await waitFor(() => expect(names(mark).filter((n) => n === "gr:graph-first-paint")).toHaveLength(2));
  });

  it("marks gr:graph-painted when the first window is painted with the snapshot, and not while loading", async () => {
    const loading = render(<GraphView model={modelOf(undefined)} repoName="tienda" stale={false} />);
    await act(async () => void (await new Promise((r) => setTimeout(r, 40))));
    expect(names(mark)).not.toContain("gr:graph-painted");
    loading.unmount();
    render(<GraphView model={model()} repoName="tienda" stale={false} />);
    await waitFor(() => expect(names(mark)).toContain("gr:graph-painted"));
  });
});
