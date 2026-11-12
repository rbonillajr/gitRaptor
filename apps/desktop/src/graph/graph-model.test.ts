// DS-US-DSK-005 T001 (§ 9.4): the pure rows of the lane graph. Red until T002.
import i18n from "i18next";
import { beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import { MIN, NOW, commits, entryOf, gapLane, modelOf, readyLane, ready, tienda, tiendaDevelop, tiendaGraph, tiendaNoBase, tiendaUnconfirmed, tiendaWithSlowLane, worktree } from "./fixtures";
import type { GraphRow } from "./graph-types";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

const kinds = (rows: readonly GraphRow[]) => rows.map((r) => r.kind);
const only = <K extends GraphRow["kind"]>(rows: readonly GraphRow[], kind: K) => rows.filter((r): r is Extract<GraphRow, { kind: K }> => r.kind === kind);

describe("graphModel", () => {
  it("one lane per worktree except the trunk, each with its agent name", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()));
    expect(model.state).toBe("ready");
    expect(model.baseName).toBe("main");
    expect(model.lanes.map((l) => l.key)).toEqual(["key-feat-login", "key-feat-pagos"]);
    expect(model.lanes.map((l) => l.column)).toEqual([0, 1]);
    expect(model.lanes.map((l) => l.branchText)).toEqual(["feat-login", "feat-pagos"]);
    expect(model.lanes.map((l) => l.agentLabel)).toEqual(["Claude Code · feat-login", "Claude Code · feat-pagos"]);
    expect(model.lanes.every((l) => !l.muted)).toBe(true);
  });

  it("a lane without a card says no agent and is muted", () => {
    const model = modelOf(tienda({ sessions: [] }), entryOf(tiendaGraph()));
    expect(model.lanes.every((l) => l.muted && l.colorSlot === null)).toBe(true);
    expect(model.lanes.map((l) => l.agentLabel)).toEqual(["No agent", "No agent"]);
  });

  it("every commit author is unattributed and never human", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()));
    const list = only(model.rows, "commit");
    expect(list).toHaveLength(4);
    for (const row of list) {
      expect(row.a11yLabel).toContain("You or another (unattributed)");
      expect(row.a11yLabel.toLowerCase()).not.toContain("human");
      expect(row.oid).toMatch(/^[0-9a-f]{40}$/);
    }
    expect(JSON.stringify(model).toLowerCase()).not.toContain("human");
  });

  it("a missing base gives no lanes and names the base", () => {
    const model = modelOf(tiendaDevelop(), entryOf(tiendaGraph()));
    expect(model.state).toBe("base-missing");
    expect(model.baseName).toBe("develop");
    expect(model.lanes).toEqual([]);
    expect(only(model.rows, "commit")).toEqual([]);
    expect(only(model.rows, "base")[0]?.name).toBe("develop");
  });

  it("an unconfirmed base gives no lanes and says how to see them", () => {
    const model = modelOf(tiendaUnconfirmed(), entryOf(tiendaGraph()));
    expect(model.state).toBe("base-unconfirmed");
    expect(model.baseName).toBe("main");
    expect(model.lanes).toEqual([]);
    expect(kinds(model.rows)).toEqual(["wip", "base"]);
  });

  it("an invalid base says no base branch", () => {
    const model = modelOf(tiendaNoBase(), null);
    expect(model.state).toBe("no-base");
    expect(model.baseName).toBeNull();
    expect(model.lanes).toEqual([]);
  });

  it("a graph the engine could not compute with repo-unavailable says the repo cannot be read", () => {
    const graph = tiendaGraph({ base: { state: "not-computable", reason: "repo-unavailable", name: null }, lanes: [], lanesTotal: 0 });
    expect(modelOf(tienda(), entryOf(graph)).state).toBe("repo-unavailable");
  });

  it("one wip row per worktree with changes, the trunk on the base column", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()));
    const wip = only(model.rows, "wip");
    expect(wip.map((r) => [r.laneKey, r.files])).toEqual([
      ["base", 3],
      ["key-feat-pagos", 2],
    ]);
    expect(model.rows.slice(0, 2).map((r) => r.kind)).toEqual(["wip", "wip"]);
    expect(wip[0]?.a11yLabel).toContain("3 uncommitted files");
  });

  it("without the graph every lane is a summary row with its ahead and behind", () => {
    const model = modelOf(tienda(), null);
    const lanes = only(model.rows, "lane");
    expect(lanes.map((r) => r.laneKey)).toEqual(["key-feat-login", "key-feat-pagos"]);
    expect(lanes[1]?.syncText).toBe("3 ahead and 1 behind main");
    expect(lanes[1]?.note).toBe("loading commits…");
    expect(only(model.rows, "commit")).toEqual([]);
  });

  it("an engine without graph.lanes keeps the summary rows with the unsupported note", () => {
    const model = modelOf(tienda(), entryOf(null, "unsupported"));
    expect(model.note).toBe("unsupported");
    expect(only(model.rows, "lane")).toHaveLength(2);
  });

  it("commits interleave by committed time", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()));
    const list = only(model.rows, "commit");
    expect(list.map((r) => r.summary)).toEqual(["Add the payment form", "Add the login form", "Validate the card number", "Scaffold the payments page"]);
    expect(list.map((r) => r.committedUtcMs)).toEqual([...list.map((r) => r.committedUtcMs)].sort((a, b) => b - a));
    expect(list.filter((r) => r.head).map((r) => r.laneKey).sort()).toEqual(["key-feat-login", "key-feat-pagos"]);
  });

  it("each lane ends in a fork row with the first seven digits of its merge-base", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()));
    const forks = only(model.rows, "fork");
    expect(forks.map((r) => r.laneKey).sort()).toEqual(["key-feat-login", "key-feat-pagos"]);
    expect(forks.every((r) => r.a11yLabel.includes("a1b2c3d"))).toBe(true);
    for (const f of forks) {
      const last = model.rows.map((r, i) => [r, i] as const).filter(([r]) => r.kind === "commit" && r.laneKey === f.laneKey).at(-1);
      expect(model.rows.indexOf(f)).toBeGreaterThan(last?.[1] ?? Infinity);
    }
  });

  it("a lane with 72 commits shows 50 and 22 more", () => {
    const graph = tiendaGraph({ lanes: [readyLane("feat-pagos", commits(50, "p", NOW), { count: 72, exact: true })], lanesTotal: 1 });
    const model = modelOf(tienda(), entryOf(graph));
    expect(only(model.rows, "commit").filter((r) => r.laneKey === "key-feat-pagos")).toHaveLength(50);
    const more = only(model.rows, "more");
    expect(more).toHaveLength(1);
    expect(more[0]).toMatchObject({ laneKey: "key-feat-pagos", count: 22, exact: true });
    expect(more[0]?.a11yLabel).toContain("22 more commits");
    const lane = model.lanes.find((l) => l.key === "key-feat-pagos");
    expect(lane).toMatchObject({ shown: 50, total: { count: 72, exact: true } });
  });

  it("an inexact total says at least", () => {
    const graph = tiendaGraph({ lanes: [readyLane("feat-pagos", commits(50, "p", NOW), { count: 72, exact: false })], lanesTotal: 1 });
    const more = only(modelOf(tienda(), entryOf(graph)).rows, "more");
    expect(more[0]).toMatchObject({ count: 22, exact: false });
  });

  it("a too-slow lane keeps its summary row with the reason", () => {
    const { fleet, graph } = tiendaWithSlowLane();
    const model = modelOf(fleet, entryOf(graph));
    const slow = only(model.rows, "lane").find((r) => r.laneKey === "key-feat-lento");
    expect(slow?.note).toBe("took too long");
    expect(slow?.syncText).toBe("5 ahead and 0 behind main");
    expect(only(model.rows, "commit").some((r) => r.laneKey === "key-feat-lento")).toBe(false);
  });

  it("every lane gap has its own reason", () => {
    const reasons = ["worktree-unavailable", "no-commits", "no-merge-base", "too-slow", "unreadable"] as const;
    const expected = ["worktree unavailable", "no commits", "no common commit with the base", "took too long", "unreadable now"];
    reasons.forEach((reason, i) => {
      const graph = tiendaGraph({ lanes: [gapLane("feat-pagos", reason), readyLane("feat-login", [], { count: 0, exact: true })], lanesTotal: 2 });
      const row = only(modelOf(tienda(), entryOf(graph)).rows, "lane").find((r) => r.laneKey === "key-feat-pagos");
      expect(row?.note).toBe(expected[i]);
    });
  });

  it("a cut lane says too many lanes", () => {
    const graph = tiendaGraph({ lanes: [readyLane("feat-pagos", commits(3, "p", NOW))], lanesTotal: 2, cut: "lanes" });
    const model = modelOf(tienda(), entryOf(graph));
    expect(model.note).toBe("cut");
    const cut = only(model.rows, "lane").find((r) => r.laneKey === "key-feat-login");
    expect(cut?.note).toBe("not shown: too many lanes");
  });

  it("a lane of the graph that the fleet does not know is ignored", () => {
    const graph = tiendaGraph({ lanes: [...tiendaGraph().lanes, readyLane("feat-fantasma", commits(2, "f", NOW - 5 * MIN, 9000))], lanesTotal: 3 });
    const model = modelOf(tienda(), entryOf(graph));
    expect(model.lanes.map((l) => l.key)).not.toContain("key-feat-fantasma");
    expect(only(model.rows, "commit").map((r) => r.summary).join(" ")).not.toContain("f 1");
  });

  it("no lane with its own commits is an empty graph that still lists the lanes", () => {
    const fleet = tienda({
      worktrees: [
        worktree("tienda", { main: true, path: "/w/tienda", status: ready() }),
        worktree("feat-pagos", { status: ready({ head: { kind: "branch", name: "feat-pagos" } }) }),
        worktree("feat-login", { status: ready({ head: { kind: "branch", name: "feat-login" } }) }),
      ],
    });
    const graph = tiendaGraph({ lanes: [readyLane("feat-pagos", [], { count: 0, exact: true }), readyLane("feat-login", [], { count: 0, exact: true })] });
    const model = modelOf(fleet, entryOf(graph));
    expect(model.empty).toBe(true);
    expect(model.lanes).toHaveLength(2);
  });

  it("the accessible name carries the visible parts and what laneA11y adds", () => {
    const model = modelOf(tienda(), entryOf(tiendaGraph()), (key) => (key === "key-feat-pagos" ? "predicted conflict with feat-login" : null));
    const pagos = only(model.rows, "commit").find((r) => r.laneKey === "key-feat-pagos");
    expect(pagos?.a11yLabel).toContain("feat-pagos, commit ");
    expect(pagos?.a11yLabel).toContain("predicted conflict with feat-login");
    const login = only(model.rows, "commit").find((r) => r.laneKey === "key-feat-login");
    expect(login?.a11yLabel).not.toContain("predicted conflict");
  });

  it("a detached head names the folder, an unborn branch says no commits", () => {
    const fleet = tienda({
      worktrees: [
        worktree("tienda", { main: true, path: "/w/tienda", status: ready() }),
        worktree("feat-pagos", { status: ready({ head: { kind: "detached" } }) }),
        worktree("feat-login", { status: ready({ head: { kind: "unborn", name: "feat-login" } }) }),
      ],
    });
    const model = modelOf(fleet, null);
    expect(model.lanes.map((l) => l.branchText)).toEqual(["feat-login (no commits)", "feat-pagos"]);
  });
});
