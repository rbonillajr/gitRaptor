// D8-MODEL (DS-US-DSK-008 D12, DS-US-CKP-008 A5, E1, E2): which conflicts are new. `nextAlerts` is pure: a conflict
// is new when its pair becomes `level: conflict` in `state: current` and the repo set does not know it; the first
// snapshot of the repo seeds; `removed`, a current pair without conflict, or absence from a complete snapshot forget
// it; `recalculating` and a truncated snapshot keep it; `closed` forgets the repo.
import { describe, expect, it } from "vitest";
import type { PairDto } from "@/client/engine-types";
import { pair } from "@/fleet/fixtures";
import { nextAlerts } from "./alerts";

type Known = ReadonlyMap<string, "seeded" | "alerted">;

const conflict = (id: string, over: Partial<PairDto> = {}): PairDto =>
  pair("feat-pagos", "feat-login", { pairId: id, state: "current", level: "conflict", computedAtUtcMs: 1_000, ...over });
const known = (entries: Array<[string, "seeded" | "alerted"]> = []): Known => new Map(entries);
const snapshot = (pairs: PairDto[], over: { truncated?: boolean; first?: boolean } = {}) =>
  ({ kind: "snapshot", pairs, truncated: over.truncated ?? false, first: over.first ?? false }) as const;
const changed = (pairs: PairDto[], removed: string[] = []) => ({ kind: "changed", pairs, removed }) as const;

describe("nextAlerts (A5, E1, E2)", () => {
  it("conflicts already there when the repo opens do not alert", () => {
    const out = nextAlerts(known(), snapshot([conflict("p1"), conflict("p2"), conflict("p3", { level: "overlap" })], { first: true }));
    expect(out.raised, "the first snapshot of the repo seeds, it never alerts").toEqual([]);
    expect([...out.state.entries()].sort(), "the conflicts are known as seeded").toEqual([["p1", "seeded"], ["p2", "seeded"]]);
  });

  it("a conflict that appears in a change alerts once", () => {
    const first = nextAlerts(known([["p1", "seeded"]]), changed([conflict("p2")]));
    expect(first.raised, "p2 is a new conflict").toEqual(["p2"]);
    expect(first.state.get("p2"), "and it is marked alerted").toBe("alerted");
    expect(first.state.get("p1"), "the seeded one stays").toBe("seeded");
    const again = nextAlerts(first.state, changed([conflict("p2", { rev: 9 })]));
    expect(again.raised, "the same pair does not alert twice").toEqual([]);
  });

  it("only a conflict in a current pair alerts", () => {
    const out = nextAlerts(
      known(),
      changed([conflict("o", { level: "overlap" }), conflict("c", { state: "calculating", level: null }), conflict("r", { state: "recalculating" }), conflict("ok")]),
    );
    expect(out.raised, "overlap, calculating and an unknown recalculating conflict do not raise; the current conflict does").toEqual(["ok"]);
  });

  it("recalculating back to current does not alert", () => {
    const start = known([["p1", "alerted"]]);
    const recalculating = nextAlerts(start, changed([conflict("p1", { state: "recalculating", rev: 2 })]));
    expect(recalculating.raised).toEqual([]);
    expect(recalculating.state.get("p1"), "a recalculating pair is still known").toBe("alerted");
    const current = nextAlerts(recalculating.state, changed([conflict("p1", { rev: 3 })]));
    expect(current.raised, "recalculating -> current is the same conflict").toEqual([]);
  });

  it("a resync only alerts unknown conflicts", () => {
    const out = nextAlerts(known([["p1", "seeded"], ["p2", "alerted"]]), snapshot([conflict("p1"), conflict("p2"), conflict("p3")]));
    expect(out.raised, "after a resync, only the conflict the set did not know").toEqual(["p3"]);
    expect(out.state.get("p3")).toBe("alerted");
  });

  it("absent from a truncated snapshot stays known", () => {
    const start = known([["p1", "alerted"], ["p2", "seeded"]]);
    const truncated = nextAlerts(start, snapshot([conflict("p2")], { truncated: true }));
    expect([...truncated.state.keys()].sort(), "a truncated snapshot does not forget what it does not list").toEqual(["p1", "p2"]);
    const complete = nextAlerts(start, snapshot([conflict("p2")]));
    expect([...complete.state.keys()], "a complete snapshot forgets the absent pair").toEqual(["p2"]);
  });

  it("removed then back alerts again", () => {
    const start = known([["p1", "alerted"]]);
    const gone = nextAlerts(start, changed([], ["p1"]));
    expect(gone.state.has("p1"), "removed forgets the pair").toBe(false);
    const back = nextAlerts(gone.state, changed([conflict("p1", { rev: 5 })]));
    expect(back.raised, "a conflict that comes back is new again").toEqual(["p1"]);
  });

  it("a current pair without conflict forgets it", () => {
    const start = known([["p1", "alerted"]]);
    const clean = nextAlerts(start, changed([conflict("p1", { level: "overlap" })]));
    expect(clean.state.has("p1"), "a current pair that is not a conflict anymore leaves the set").toBe(false);
    expect(nextAlerts(clean.state, changed([conflict("p1")])).raised, "and alerts again when it returns").toEqual(["p1"]);
  });

  it("closed forgets the repo and never mutates the input", () => {
    const start = known([["p1", "alerted"]]);
    const copy = [...start.entries()];
    expect(nextAlerts(start, { kind: "closed" }).state.size, "repoClosed forgets the repo").toBe(0);
    nextAlerts(start, changed([conflict("p2")]));
    expect([...start.entries()], "the previous state is untouched").toEqual(copy);
  });
});
