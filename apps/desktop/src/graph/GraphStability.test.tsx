// Review fixes: ⌥⌘T keeps the focus inside the view (D21) and the one-second clock does not repaint the canvas (NFR-13.8).
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { GraphView } from "./GraphView";
import { entryOf, modelOf, tienda, tiendaGraph } from "./fixtures";
import { useGraphPrefs } from "./graph-prefs";

const model = () => modelOf(tienda(), entryOf(tiendaGraph()));

beforeEach(async () => {
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Macintosh");
  useGraphPrefs.setState({ view: "graph" });
  await act(async () => void (await i18n.changeLanguage("en")));
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("graph view focus", () => {
  it("keeps the focus inside the view when the chord swaps it from a row", () => {
    render(<GraphView model={model()} repoName="tienda" stale={false} />);
    const row = screen.getAllByRole("row")[1];
    expect(row).toBeDefined();
    act(() => (row?.querySelector<HTMLElement>("[tabindex]") ?? (row as HTMLElement)).focus());
    const before = document.activeElement;
    expect(before).not.toBe(document.body);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "t", code: "KeyT", metaKey: true, altKey: true });
    expect(useGraphPrefs.getState().view).toBe("table");
    expect(document.activeElement).not.toBe(document.body);
    expect(document.activeElement?.closest("section[data-graph]")).not.toBeNull();
  });
});

describe("graph canvas repaint", () => {
  it("does not repaint while the clock rebuilds an equal model", async () => {
    vi.useFakeTimers();
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    const ctx = new Proxy({}, { get: (_t, k) => (k === "canvas" ? {} : vi.fn()), set: () => true });
    const getContext = vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
    const { rerender } = render(<GraphView model={model()} repoName="tienda" stale={false} />);
    const flush = () => act(() => void frames.splice(0).forEach((cb) => cb(0)));
    flush();
    const painted = getContext.mock.calls.length;
    expect(painted).toBeGreaterThan(0);
    for (let s = 0; s < 5; s += 1) {
      act(() => void vi.advanceTimersByTime(1000));
      rerender(<GraphView model={model()} repoName="tienda" stale={false} />);
      flush();
    }
    expect(getContext.mock.calls.length, "no frame was drawn without a real change").toBe(painted);
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });
});
