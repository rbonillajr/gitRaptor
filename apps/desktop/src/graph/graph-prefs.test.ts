// DS-US-DSK-005 T001 (§ 9.4): column preferences, their validation and the folding by width. Red until T004.
import { beforeEach, describe, expect, it } from "vitest";
import { folded, useGraphPrefs, visibleColumns, type GraphPrefs } from "./graph-prefs";

const ALL: GraphPrefs = { columns: { branch: true, graph: true, author: true, date: true, sha: true }, view: "graph" };
const without = (...ids: Array<keyof GraphPrefs["columns"]>): GraphPrefs => ({ ...ALL, columns: { ...ALL.columns, ...Object.fromEntries(ids.map((id) => [id, false])) } });

beforeEach(() => {
  localStorage.clear();
});

/** The `merge` the store hands to `persist`: what it keeps of a stored value. */
const merged = (stored: unknown): GraphPrefs => {
  const merge = useGraphPrefs.persist.getOptions().merge;
  if (!merge) throw new Error("the store has no merge");
  const { columns, view } = merge(stored, useGraphPrefs.getInitialState());
  return { columns, view };
};

describe("graph prefs store", () => {
  it("persists columns and view under gitraptor.graph.v1 version 1", () => {
    useGraphPrefs.getState().setColumn("sha", false);
    useGraphPrefs.getState().setView("table");
    const raw = localStorage.getItem("gitraptor.graph.v1");
    expect(raw).not.toBeNull();
    const stored: unknown = JSON.parse(raw ?? "{}");
    expect(stored).toMatchObject({ version: 1, state: { columns: { sha: false, branch: true }, view: "table" } });
    expect(useGraphPrefs.persist.getOptions().name).toBe("gitraptor.graph.v1");
  });

  it("rehydrates from localStorage", async () => {
    localStorage.setItem("gitraptor.graph.v1", JSON.stringify({ version: 1, state: { columns: { author: false }, view: "table" } }));
    await useGraphPrefs.persist.rehydrate();
    expect(useGraphPrefs.getState().columns.author).toBe(false);
    expect(useGraphPrefs.getState().columns.sha).toBe(true);
    expect(useGraphPrefs.getState().view).toBe("table");
  });

  it("ignores unknown keys, wrong types and broken json", () => {
    expect(merged({ columns: { sha: "no", date: 0, message: false, evil: true, author: false }, view: "carousel", extra: 1 })).toEqual({
      columns: { branch: true, graph: true, author: false, date: true, sha: true },
      view: "graph",
    });
    expect(merged("{broken")).toEqual(ALL);
    expect(merged(null)).toEqual(ALL);
    expect(merged({ columns: [true, false], view: { a: 1 } })).toEqual(ALL);
    expect(merged({ columns: { __proto__: { sha: false } } })).toEqual(ALL);
  });

  it("reset brings every column back and keeps the view", () => {
    useGraphPrefs.getState().setColumn("date", false);
    useGraphPrefs.getState().setView("table");
    useGraphPrefs.getState().resetColumns();
    expect(useGraphPrefs.getState().columns).toEqual(ALL.columns);
    expect(useGraphPrefs.getState().view).toBe("table");
  });
});

describe("visibleColumns", () => {
  it("it folds sha, author, date and branch in that order until message has 220 px", () => {
    // Fixed widths: Branch 195, Graph 175, Author 190, Date 120, Sha 84; message needs 220.
    expect(visibleColumns(ALL, 984)).toEqual(["branch", "graph", "message", "author", "date", "sha"]);
    expect(visibleColumns(ALL, 983)).toEqual(["branch", "graph", "message", "author", "date"]);
    expect(visibleColumns(ALL, 899)).toEqual(["branch", "graph", "message", "date"]);
    expect(visibleColumns(ALL, 709)).toEqual(["branch", "graph", "message"]);
    expect(visibleColumns(ALL, 589)).toEqual(["graph", "message"]);
  });

  it("at 788 px it keeps branch, graph, message and date", () => {
    expect(visibleColumns(ALL, 788)).toEqual(["branch", "graph", "message", "date"]);
  });

  it("at 708 px it keeps branch, graph and message", () => {
    expect(visibleColumns(ALL, 708)).toEqual(["branch", "graph", "message"]);
  });

  it("graph and message never fold", () => {
    expect(visibleColumns(ALL, 0)).toEqual(["graph", "message"]);
    expect(visibleColumns(without("graph"), 2000)).toContain("graph");
  });

  it("a hidden column stays hidden whatever the width", () => {
    expect(visibleColumns(without("sha", "author"), 2000)).toEqual(["branch", "graph", "message", "date"]);
  });

  it("a folded column keeps its saved toggle", () => {
    expect(folded(ALL, 788)).toEqual(["author", "sha"]);
    expect(folded(without("sha"), 788)).toEqual(["author"]);
    expect(folded(ALL, 2000)).toEqual([]);
    expect(ALL.columns.sha).toBe(true);
    expect(ALL.columns.author).toBe(true);
  });
});
