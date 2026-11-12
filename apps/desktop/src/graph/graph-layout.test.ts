// DS-US-DSK-005 T001 (§ 9.4): the geometry of the lane canvas. Red until T003.
import i18n from "i18next";
import { beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import { MIN, NOW, commits, counted, entryOf, modelOf, ready, readyLane, tienda, tiendaGraph, worktree } from "./fixtures";
import { graphLayout } from "./graph-layout";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

/** A repo with `n` lanes named lane-00 …, each with one commit. */
const gapOf = (g: ReturnType<typeof graphLayout>) => (g.laneX.get("key-lane-01") ?? 0) - (g.laneX.get("key-lane-00") ?? 0);

function manyLanes(n: number) {
  const names = Array.from({ length: n }, (_, i) => `lane-${String(i).padStart(2, "0")}`);
  const fleet = tienda({
    worktrees: [
      worktree("tienda", { main: true, path: "/w/tienda", status: ready() }),
      ...names.map((name) => worktree(name, { status: ready({ head: { kind: "branch", name }, divergence: counted(1, 0) }) })),
    ],
    sessions: [],
  });
  const graph = tiendaGraph({ lanes: names.map((name, i) => readyLane(name, commits(1, name, NOW - i * MIN, 100 + i))), lanesTotal: n });
  return modelOf(fleet, entryOf(graph));
}

describe("graphLayout", () => {
  it("puts the base at half a gap and each lane one gap further", () => {
    const g = graphLayout(modelOf(tienda(), entryOf(tiendaGraph())), 175);
    expect(g.laneX.get("base")).toBe(8);
    expect(g.laneX.get("key-feat-login")).toBe(8 + 16);
    expect(g.laneX.get("key-feat-pagos")).toBe(8 + 32);
    expect(g.nodeRadius).toBe(4);
    expect(g.clipped).toBe(0);
  });

  it("lane columns stay put when a lane changes fleet group", () => {
    const before = graphLayout(modelOf(tienda(), entryOf(tiendaGraph())), 175);
    const idle = tienda({ sessions: [] });
    const after = graphLayout(modelOf(idle, entryOf(tiendaGraph())), 175);
    for (const key of ["key-feat-login", "key-feat-pagos"]) expect(after.laneX.get(key)).toBe(before.laneX.get(key));
  });

  it("a lane spans from its first row to its fork row, the base to the last fork", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()));
    const g = graphLayout(model, 175);
    const fork = (key: string) => model.rows.findIndex((r) => r.kind === "fork" && r.laneKey === key);
    expect(g.laneSpan.get("key-feat-pagos")?.last).toBe(fork("key-feat-pagos"));
    expect(g.laneSpan.get("key-feat-pagos")?.first).toBe(model.rows.findIndex((r) => r.kind === "wip" && r.laneKey === "key-feat-pagos"));
    expect(g.laneSpan.get("key-feat-login")?.first).toBe(model.rows.findIndex((r) => r.kind === "commit" && r.laneKey === "key-feat-login"));
    expect(g.laneSpan.get("base")?.last).toBe(Math.max(fork("key-feat-pagos"), fork("key-feat-login")));
    expect(g.rowIndex.size).toBe(model.rows.length);
  });

  it("the gap shrinks to fit the column and the node radius follows below 10 px", () => {
    const ten = graphLayout(manyLanes(10), 175);
    expect(ten.laneX.get("key-lane-09")).toBeLessThanOrEqual(175);
    expect(gapOf(ten)).toBeLessThan(16);
    expect(ten.nodeRadius).toBe(4);
    expect(graphLayout(manyLanes(18), 175).nodeRadius).toBe(3);
  });

  it("the gap never goes below 8 px", () => {
    const g = graphLayout(manyLanes(30), 175);
    expect(gapOf(g)).toBe(8);
  });

  it("lanes past the column are clipped and counted", () => {
    const model = manyLanes(30);
    const g = graphLayout(model, 175);
    const drawn = model.lanes.filter((l) => g.laneX.has(l.key)).length;
    expect(drawn).toBe(20);
    expect(g.clipped).toBe(10);
    for (const x of g.laneX.values()) expect(x).toBeLessThanOrEqual(175);
  });
});
