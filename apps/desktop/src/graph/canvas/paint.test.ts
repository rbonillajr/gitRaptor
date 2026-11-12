// DS-US-DSK-005 T003 (§ 9.4): the canvas painter with a fake 2D context that records every call.
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { entryOf, modelOf, tienda, tiendaGraph } from "../fixtures";
import { graphLayout } from "../graph-layout";
import type { GraphColors, GraphLayer } from "../graph-types";
import { createPainter, paint } from "./paint";

const COLORS: GraphColors = {
  base: "token-base",
  lane: ["token-1", "token-2", "token-3", "token-4", "token-5", "token-6", "token-7"],
  muted: "token-muted",
  danger: "token-danger",
};

type Call = [string, ...unknown[]];

/** A context that logs method calls as `[name, ...args]` and property writes as `["set:name", value]`. */
function fakeContext(): { ctx: CanvasRenderingContext2D; calls: Call[] } {
  const calls: Call[] = [];
  const ctx = new Proxy(
    {},
    {
      get: (_t, name: string) => (...args: unknown[]) => void calls.push([name, ...args]),
      set: (_t, name: string, value: unknown) => {
        calls.push([`set:${name}`, value]);
        return true;
      },
    },
  );
  return { ctx: ctx as CanvasRenderingContext2D, calls };
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
});
afterEach(() => vi.unstubAllGlobals());

const scene = () => {
  const model = modelOf(tienda(), entryOf(tiendaGraph()));
  return { model, geometry: graphLayout(model, 175), window: { first: 0, last: model.rows.length - 1 } };
};

describe("paint", () => {
  it("a registered layer draws between lane lines and nodes", () => {
    const { model, geometry, window } = scene();
    const { ctx, calls } = fakeContext();
    const layer: GraphLayer = { id: "probe", draw: (c) => c.fillRect(0, 0, 1, 1) };
    paint(ctx, geometry, model, window, [layer], COLORS);
    const names = calls.map((c) => c[0]);
    const probe = names.indexOf("fillRect");
    expect(probe).toBeGreaterThan(-1);
    expect(names.slice(0, probe)).toContain("stroke");
    expect(names.slice(0, probe)).not.toContain("arc");
    expect(names.slice(probe)).toContain("arc");
  });

  it("layers draw in registration order with the geometry, the window and the colours", () => {
    const { model, geometry, window } = scene();
    const order: string[] = [];
    const seen: unknown[] = [];
    const layer = (id: string): GraphLayer => ({
      id,
      draw: (_c, g, w, c) => {
        order.push(id);
        seen.push(g, w, c);
      },
    });
    paint(fakeContext().ctx, geometry, model, window, [layer("a"), layer("b")], COLORS);
    expect(order).toEqual(["a", "b"]);
    expect(seen.slice(0, 3)).toEqual([geometry, window, COLORS]);
  });

  it("no colour literal reaches the context", () => {
    const { model, geometry, window } = scene();
    const { ctx, calls } = fakeContext();
    paint(ctx, geometry, model, window, [], COLORS);
    const styles = calls.filter(([name]) => name === "set:strokeStyle" || name === "set:fillStyle").map(([, value]) => value);
    expect(styles.length).toBeGreaterThan(0);
    const allowed = new Set<unknown>([COLORS.base, COLORS.muted, COLORS.danger, ...COLORS.lane]);
    for (const style of styles) expect(allowed.has(style), `${String(style)} is not a token value`).toBe(true);
  });

  it("paints no text", () => {
    const { model, geometry, window } = scene();
    const { ctx, calls } = fakeContext();
    paint(ctx, geometry, model, window, [], COLORS);
    expect(calls.map((c) => c[0])).not.toContain("fillText");
    expect(calls.map((c) => c[0])).not.toContain("strokeText");
  });

  it("paints only the rows of the window", () => {
    const { model, geometry } = scene();
    const all = fakeContext();
    paint(all.ctx, geometry, model, { first: 0, last: model.rows.length - 1 }, [], COLORS);
    const some = fakeContext();
    paint(some.ctx, geometry, model, { first: 0, last: 1 }, [], COLORS);
    const nodes = (calls: Call[]) => calls.filter((c) => c[0] === "arc").length;
    expect(nodes(some.calls)).toBeLessThan(nodes(all.calls));
    expect(some.calls[0]).toEqual(["clearRect", 0, 0, 175, 2 * geometry.rowHeight]);
  });

  it("a lane without an agent takes the muted colour and one with a slot takes its lane colour", () => {
    const withAgents = scene();
    const a = fakeContext();
    paint(a.ctx, withAgents.geometry, withAgents.model, withAgents.window, [], COLORS);
    expect(a.calls.some(([n, v]) => n === "set:strokeStyle" && v === "token-1")).toBe(true);
    const model = modelOf(tienda({ sessions: [] }), entryOf(tiendaGraph()));
    const b = fakeContext();
    paint(b.ctx, graphLayout(model, 175), model, { first: 0, last: model.rows.length - 1 }, [], COLORS);
    const styles = b.calls.filter(([n]) => n === "set:strokeStyle").map(([, v]) => v);
    expect(styles).toContain("token-muted");
    expect(styles.some((s) => typeof s === "string" && /^token-\d$/.test(s))).toBe(false);
  });
});

describe("createPainter", () => {
  it("nothing is painted without a change", () => {
    const raf = vi.fn((cb: FrameRequestCallback) => {
      cb(0);
      return 1;
    });
    vi.stubGlobal("requestAnimationFrame", raf);
    const draw = vi.fn();
    createPainter(draw);
    expect(raf).not.toHaveBeenCalled();
    expect(draw).not.toHaveBeenCalled();
  });

  it("several requests in a frame paint once and an idle frame schedules nothing more", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    const draw = vi.fn();
    const painter = createPainter(draw);
    painter.request();
    painter.request();
    painter.request();
    expect(frames).toHaveLength(1);
    frames[0]?.(0);
    expect(draw).toHaveBeenCalledTimes(1);
    expect(frames).toHaveLength(1);
    painter.request();
    expect(frames).toHaveLength(2);
  });

  it("cancel drops the pending frame", () => {
    const cancel = vi.fn();
    vi.stubGlobal("requestAnimationFrame", () => 7);
    vi.stubGlobal("cancelAnimationFrame", cancel);
    const painter = createPainter(vi.fn());
    painter.request();
    painter.cancel();
    expect(cancel).toHaveBeenCalledWith(7);
  });
});
