// DS-US-DSK-005 T011 (§ 9.4): the ⚡ line of the conflict layer, with a fake 2D context.
import i18n from "i18next";
import { beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { ConflictBandModel } from "@/conflicts";
import { MIN, NOW, commits, counted, entryOf, modelOf, ready, readyLane, tienda, tiendaGraph, worktree } from "./fixtures";
import { conflictLayer } from "./conflict-layer";
import { graphLayout } from "./graph-layout";
import type { GraphColors, GraphGeometry } from "./graph-types";

const COLORS: GraphColors = { base: "t-base", lane: ["t1", "t2", "t3", "t4", "t5", "t6", "t7"], muted: "t-muted", danger: "t-danger" };

const band = (over: Partial<ConflictBandModel> = {}): ConflictBandModel => ({
  pairId: "p",
  level: "conflict",
  a: { kind: "worktree", key: "key-feat-pagos" },
  b: { kind: "worktree", key: "key-feat-login" },
  aLabel: "feat-pagos",
  bLabel: "feat-login",
  firstFile: null,
  moreText: null,
  freshnessText: "",
  stale: false,
  isNew: false,
  a11yLabel: "conflict",
  ...over,
});

type Call = [string, ...unknown[]];
function draw(bands: ConflictBandModel[], geometry: GraphGeometry, window: { first: number; last: number }): Call[] {
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
  ) as CanvasRenderingContext2D;
  conflictLayer(bands).draw(ctx, geometry, window, COLORS);
  return calls;
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

const scene = () => {
  const model = modelOf(tienda(), entryOf(tiendaGraph()));
  const geometry = graphLayout(model, 175);
  return { model, geometry, window: { first: 0, last: model.rows.length - 1 } };
};
const lines = (calls: Call[]) => calls.filter(([n]) => n === "lineTo");
const dash = (calls: Call[]) => calls.filter(([n]) => n === "setLineDash").map(([, d]) => d);

describe("conflictLayer", () => {
  it("has the id conflicts", () => {
    expect(conflictLayer([]).id).toBe("conflicts");
  });

  it("a conflict draws one danger line between its two lanes at the newer head row", () => {
    const { model, geometry, window } = scene();
    const calls = draw([band()], geometry, window);
    const xa = geometry.laneX.get("key-feat-pagos");
    const xb = geometry.laneX.get("key-feat-login");
    // pagos' newest commit (-10 min) is above login's (-20 min): the line is at the pagos head row.
    const head = model.rows.findIndex((r) => r.kind === "commit" && r.laneKey === "key-feat-pagos");
    const y = (head + 0.5) * geometry.rowHeight;
    expect(calls).toContainEqual(["set:strokeStyle", "t-danger"]);
    expect(calls).toContainEqual(["moveTo", xa, y]);
    expect(lines(calls)).toEqual([["lineTo", xb, y]]);
    expect(calls.filter(([n]) => n === "stroke")).toHaveLength(1);
  });

  it("a stale or non-contiguous pair draws a dashed line", () => {
    const { geometry, window } = scene();
    expect(dash(draw([band()], geometry, window))[0], "a fresh contiguous pair is solid").toEqual([]);
    expect(dash(draw([band({ stale: true })], geometry, window))[0], "a stale pair is dashed").not.toEqual([]);

    // Three lanes: the pair of the outer two has the middle one between its columns.
    const names = ["lane-a", "lane-b", "lane-c"];
    const fleet = tienda({
      worktrees: [worktree("tienda", { main: true, path: "/w/tienda", status: ready() }), ...names.map((n) => worktree(n, { status: ready({ head: { kind: "branch", name: n }, divergence: counted(1, 0) }) }))],
      sessions: [],
    });
    const graph = tiendaGraph({ lanes: names.map((n, i) => readyLane(n, commits(1, n, NOW - i * MIN, 50 + i))), lanesTotal: 3 });
    const model = modelOf(fleet, entryOf(graph));
    const g = graphLayout(model, 175);
    const outer = band({ a: { kind: "worktree", key: "key-lane-a" }, b: { kind: "worktree", key: "key-lane-c" } });
    expect(dash(draw([outer], g, { first: 0, last: model.rows.length - 1 }))[0]).not.toEqual([]);
    const adjacent = band({ a: { kind: "worktree", key: "key-lane-a" }, b: { kind: "worktree", key: "key-lane-b" } });
    expect(dash(draw([adjacent], g, { first: 0, last: model.rows.length - 1 }))[0]).toEqual([]);
  });

  it("a pair against the base reaches the base column", () => {
    const { geometry, window } = scene();
    const calls = draw([band({ b: { kind: "base" } })], geometry, window);
    expect(lines(calls)[0]?.[1]).toBe(geometry.laneX.get("base"));
  });

  it("an overlap draws nothing on the canvas", () => {
    const { geometry, window } = scene();
    expect(draw([band({ level: "overlap" })], geometry, window)).toEqual([]);
  });

  it("a clipped lane draws no line", () => {
    const { geometry, window } = scene();
    const clipped: GraphGeometry = { ...geometry, laneX: new Map([...geometry.laneX].filter(([key]) => key !== "key-feat-login")) };
    expect(draw([band()], clipped, window)).toEqual([]);
  });

  it("a line outside the window is not drawn", () => {
    const { model, geometry } = scene();
    const head = model.rows.findIndex((r) => r.kind === "commit" && r.laneKey === "key-feat-pagos");
    expect(draw([band()], geometry, { first: head + 1, last: model.rows.length - 1 })).toEqual([]);
  });
});
