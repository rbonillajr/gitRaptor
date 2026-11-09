// D8-MODEL (DS-US-DSK-008 D6, D8-D11, § 9.2): the pure view model of the predicted conflicts: bands, freshness lines,
// pending base and the detail of one pair. Names come from the cards (`fleetCards`), ages from `formatAge`, texts from
// the `conflicts.*` keys of the Dev Spec (en).
import i18n, { type TFunction } from "i18next";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { PairDetailDto, PairDto, RepoFleet } from "@/client/engine-types";
import { fleetCards } from "@/fleet/fleet-model";
import { NOW, pair, tienda } from "@/fleet/fixtures";
import { usePredictionView } from "./index";
import { pairDetailView, predictionView } from "./prediction-model";

const t = i18n.t.bind(i18n) as unknown as TFunction;
const AGO = 40_000;
const conflictPair = (over: Partial<PairDto> = {}): PairDto =>
  pair("feat-pagos", "feat-login", { rev: 4, state: "current", level: "conflict", computedAtUtcMs: NOW - AGO, conflictFiles: 1, ...over });
const baseOverlap = (over: Partial<PairDto> = {}): PairDto =>
  pair("feat-pagos", "base", { rev: 4, state: "current", level: "overlap", computedAtUtcMs: NOW - AGO, overlapFiles: 1, ...over });
const detail = (p: PairDto, over: Partial<PairDetailDto> = {}): PairDetailDto => ({
  pair: p,
  overlap: [],
  conflict: {
    files: [{ path: "pagos.rs", kind: "content", ours: [{ start: 40, len: 19 }], theirs: [{ start: 44, len: 18 }], hunksTruncated: false, hasMergeRule: false }],
    truncated: false,
  },
  ...over,
});
function fleetWith(pairs: PairDto[], details: PairDetailDto[] = [], over: Partial<RepoFleet["repo"]> = {}, last: Array<[string, number]> = []): RepoFleet {
  const fleet = tienda({ pairs, repo: { limits: ["overlap-includes-uncommitted", "local-only"], ...over } });
  return { ...fleet, details: new Map(details.map((d) => [d.pair.pairId, d])), lastComputed: new Map(last) };
}
const view = (fleet: RepoFleet, isNew: (id: string) => boolean = () => false) =>
  predictionView(fleet, fleetCards(fleet, NOW, true, t), NOW, t, isNew);
const detailView = (fleet: RepoFleet, pairId: string) => pairDetailView(fleet, fleetCards(fleet, NOW, true, t), pairId, NOW, t);
const label = (fleet: RepoFleet, folder: string) => fleetCards(fleet, NOW, true, t).find((c) => c.folder === folder)?.agent.label ?? "?";

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));

describe("predictionView bands", () => {
  it("a conflict pair is a band with names from the cards, the first file and the age", () => {
    const p = conflictPair({ conflictFiles: 3 });
    const fleet = fleetWith([p], [detail(p)]);
    const [band] = view(fleet).bands;
    expect(band, "one band for the conflict").toBeDefined();
    expect(band).toMatchObject({
      pairId: p.pairId,
      level: "conflict",
      a: { kind: "worktree", key: "key-feat-pagos" },
      b: { kind: "worktree", key: "key-feat-login" },
      aLabel: label(fleet, "feat-pagos"),
      bLabel: label(fleet, "feat-login"),
      firstFile: "pagos.rs",
      moreText: "and 2 more",
      freshnessText: "40 s ago",
      stale: false,
      isNew: false,
    });
    expect(band?.a11yLabel, "the accessible name reads the level, both sides, the file and the age").toBe(
      `predicted conflict between ${label(fleet, "feat-pagos")} and ${label(fleet, "feat-login")}, pagos.rs, 40 s ago`,
    );
  });

  it("the base side is named with the base branch and the isNew flag comes from the caller", () => {
    const p = baseOverlap();
    const fleet = fleetWith([p], [detail(p, { conflict: null, overlap: ["README.md"] })]);
    const [band] = view(fleet, (id) => id === p.pairId).bands;
    expect(band?.b, "the base is a SideRef of its own").toEqual({ kind: "base" });
    expect(band?.bLabel, "D8: {{base}} (main branch)").toBe("main (main branch)");
    expect(band?.level).toBe("overlap");
    expect(band?.firstFile, "the first overlap path for a warning").toBe("README.md");
    expect(band?.isNew).toBe(true);
  });

  it("a detail of another rev gives no file, and a missing detail neither", () => {
    const p = conflictPair({ rev: 5 });
    expect(view(fleetWith([p], [detail(conflictPair({ rev: 4 }))])).bands[0]?.firstFile, "the rev of the detail is not the summary's").toBeNull();
    expect(view(fleetWith([p])).bands[0]?.firstFile, "no detail yet").toBeNull();
    expect(view(fleetWith([p])).bands, "the pair is still a band without its file").toHaveLength(1);
  });

  it("conflicts come first, then warnings, then by pairId; only current and recalculating pairs are bands", () => {
    const pairs = [
      baseOverlap({ pairId: "b-overlap" }),
      conflictPair({ pairId: "z-conflict" }),
      conflictPair({ pairId: "a-conflict" }),
      conflictPair({ pairId: "calc", state: "calculating", level: null, computedAtUtcMs: null }),
      conflictPair({ pairId: "pend", state: "pending", level: null, reason: "base-unconfirmed", computedAtUtcMs: null }),
      conflictPair({ pairId: "clean", level: "none" }),
    ];
    expect(view(fleetWith(pairs)).bands.map((b) => b.pairId)).toEqual(["a-conflict", "z-conflict", "b-overlap"]);
  });

  it("a recalculating pair says outdated with its age", () => {
    const p = conflictPair({ state: "recalculating" });
    const [band] = view(fleetWith([p], [detail(p)])).bands;
    expect(band?.stale, "recalculating is stale").toBe(true);
    expect(band?.freshnessText).toBe("outdated · 40 s ago · recalculating");
  });
});

describe("predictionView lines (BR-CKP-CALC-003)", () => {
  it("a moved base says calculating with the age of the last result", () => {
    const moved = baseOverlap({ state: "calculating", level: null, computedAtUtcMs: null });
    const remembered = fleetWith([moved], [], {}, [[moved.pairId, NOW - AGO]]);
    expect(view(remembered).lines, "D10: the age of the last result, without its level").toEqual(["Calculating 1 of 1 pair · last result 40 s ago"]);
    expect(view(remembered).bands, "no band without a result").toEqual([]);
    expect(view(fleetWith([moved])).lines, "nothing remembered: only the count").toEqual(["Calculating 1 of 1 pair"]);
  });

  it("calculating counts the pairs that have no result, current ones without a time included (E4)", () => {
    const pairs = [
      conflictPair({ pairId: "p1", state: "calculating", level: null, computedAtUtcMs: null }),
      conflictPair({ pairId: "p2", state: "current", level: "overlap", computedAtUtcMs: null }),
      conflictPair({ pairId: "p3", state: "recalculating", level: "conflict", computedAtUtcMs: null }),
      conflictPair({ pairId: "p4" }),
    ];
    expect(view(fleetWith(pairs)).lines[0], "3 of 4 pairs have no result").toBe("Calculating 3 of 4 pairs");
    expect(view(fleetWith(pairs)).bands.map((b) => b.pairId), "a pair without a time is not a band").toEqual(["p4"]);
  });

  it("no pair without a result says no conflicts", () => {
    const calculating = conflictPair({ state: "calculating", level: null, computedAtUtcMs: null });
    const noTime = conflictPair({ pairId: "x", state: "current", level: "none", computedAtUtcMs: null });
    const pending = conflictPair({ pairId: "y", state: "pending", level: null, reason: "dormant", computedAtUtcMs: null });
    const clean = conflictPair({ pairId: "z", level: "none", computedAtUtcMs: NOW - 120_000 });
    for (const pairs of [[calculating], [noTime], [pending], [clean, calculating], [clean, pending]]) {
      const text = view(fleetWith(pairs)).lines.join("\n");
      expect(text, `a pair without a result never says "No predicted conflicts" (${pairs.map((p) => p.pairId)})`).not.toContain("No predicted conflicts");
    }
    expect(view(fleetWith([clean])).lines, "all current and no conflict: the age of the oldest result").toEqual(["No predicted conflicts · 2 min ago"]);
    expect(view(fleetWith([clean, baseOverlap()])).lines.join("\n"), "a warning is not 'no conflicts'").not.toContain("No predicted conflicts");
  });

  it("zero pairs show only the limits", () => {
    const out = view(fleetWith([], [], { confirmCommand: { state: "not-offered", reason: "raptor-unknown" } }));
    expect(out.bands).toEqual([]);
    expect(out.lines, "no pair, no freshness line").toEqual([]);
    expect(out.basePending).toBeNull();
    expect(out.limits, "the limits that came with the repo").toEqual(["overlap-includes-uncommitted", "local-only"]);
  });

  it("pending and not-computable pairs are grouped by reason, after calculating and before the clear line", () => {
    const pairs = [
      conflictPair({ pairId: "c1", state: "calculating", level: null, computedAtUtcMs: null }),
      conflictPair({ pairId: "p1", state: "pending", level: null, reason: "base-unconfirmed", computedAtUtcMs: null }),
      conflictPair({ pairId: "p2", state: "pending", level: null, reason: "base-unconfirmed", computedAtUtcMs: null }),
      conflictPair({ pairId: "n1", state: "not-computable", level: null, reason: "shallow", computedAtUtcMs: null }),
    ];
    const lines = view(fleetWith(pairs)).lines;
    expect(lines, "one line per group").toHaveLength(3);
    expect(lines[0]).toBe("Calculating 1 of 4 pairs");
    expect(lines[1], "pending, with the count of its reason").toMatch(/^Pending: .+ \(2 pairs\)$/);
    expect(lines[2], "not computable, with its reason").toMatch(/^Not computable: .+ \(1 pair\)$/);
    expect(lines.join("\n"), "every reason has a text in the catalog").not.toContain("conflicts.");
  });
});

describe("predictionView base pending (D11)", () => {
  const pendingBase = (over: Partial<PairDto> = {}) =>
    baseOverlap({ pairId: "base-pending", state: "pending", level: null, reason: "base-unconfirmed", computedAtUtcMs: null, ...over });
  const offered = { state: "offered", command: "'/opt/raptor' 'guard' 'confirm' '/w/tienda/.git'", withPath: true } as const;

  it("a pair against the base pending with base-unconfirmed shows the warning with the backend command", () => {
    const fleet = fleetWith([pendingBase()], [], { base: { name: "main", status: "unconfirmed" }, confirmCommand: offered });
    expect(view(fleet).basePending).toEqual({ base: "main", repo: "tienda", command: offered });
  });

  it("an unconfirmed base with no pair against it still shows the warning", () => {
    const fleet = fleetWith([conflictPair()], [], { base: { name: "main", status: "unconfirmed" }, confirmCommand: offered });
    expect(view(fleet).basePending?.base).toBe("main");
  });

  it("a base pair that leaves pending hides the warning even if the snapshot still says unconfirmed", () => {
    const fleet = fleetWith([baseOverlap()], [], { base: { name: "main", status: "unconfirmed" }, confirmCommand: offered });
    expect(view(fleet).basePending, "G2: a pair against the base that is computed proves the base is confirmed").toBeNull();
  });

  it("a confirmed base shows no warning, while a pending pair against an unconfirmed base does", () => {
    const confirmed = fleetWith([pendingBase()], [], { base: { name: "main", status: "confirmed" }, confirmCommand: offered });
    expect(view(confirmed).basePending, "the reason of the pair is what shows the warning, the status only fills the gaps").not.toBeNull();
    expect(view(fleetWith([baseOverlap()], [], { base: { name: "main", status: "confirmed" } })).basePending).toBeNull();
  });
});

describe("pairDetailView", () => {
  it("a conflict between two worktrees lists each file with the lines of each side", () => {
    const p = conflictPair();
    const fleet = fleetWith([p], [detail(p)]);
    const out = detailView(fleet, p.pairId);
    expect(out.status).toBe("default");
    expect(out.title).toBe(`⚡ ${label(fleet, "feat-pagos")} ↔ ${label(fleet, "feat-login")} are going to collide`);
    expect(out.subtitle, "folder A ↔ folder B and the age").toBe("feat-pagos ↔ feat-login · computed 40 s ago");
    expect(out.conflictFiles).toHaveLength(1);
    expect(out.conflictFiles[0]).toMatchObject({
      path: "pagos.rs",
      aText: `${label(fleet, "feat-pagos")} — lines 40–58`,
      bText: `${label(fleet, "feat-login")} — lines 44–61`,
      mergeRule: false,
    });
    expect(out.conflictFiles[0]?.kindText, "the kind has a catalog text").not.toContain("conflicts.");
    expect(out.truncated).toBe(false);
  });

  it("a range of one line says line, and truncated hunks add more ranges", () => {
    const p = conflictPair();
    const d = detail(p);
    const file = d.conflict?.files[0];
    if (!file || !d.conflict) throw new Error("fixture");
    d.conflict.files = [{ ...file, ours: [{ start: 7, len: 1 }], theirs: [{ start: 9, len: 0 }], hunksTruncated: true, hasMergeRule: true }];
    d.conflict.truncated = true;
    const fleet = fleetWith([p], [d]);
    const out = detailView(fleet, p.pairId);
    expect(out.conflictFiles[0]?.aText).toContain("line 7");
    expect(out.conflictFiles[0]?.aText, "a truncated list says there are more ranges").toContain("and more ranges");
    expect(out.conflictFiles[0]?.bText, "len 0 is also one line").toContain("line 9");
    expect(out.conflictFiles[0]?.mergeRule).toBe(true);
    expect(out.truncated).toBe(true);
  });

  it("statuses: overlap-only, against-base, loading, stale, calculating, pending, resolved and gone", () => {
    const overlap = conflictPair({ pairId: "ov", level: "overlap" });
    const against = conflictPair({ pairId: "ag", b: "base" });
    const missing = conflictPair({ pairId: "ld" });
    const stale = conflictPair({ pairId: "st", state: "recalculating" });
    const calc = conflictPair({ pairId: "ca", state: "calculating", level: null, computedAtUtcMs: null });
    const pend = conflictPair({ pairId: "pe", state: "pending", level: null, reason: "base-unconfirmed", computedAtUtcMs: null });
    const resolved = conflictPair({ pairId: "re", level: "none" });
    const fleet = fleetWith(
      [overlap, against, missing, stale, calc, pend, resolved],
      [detail(overlap, { overlap: ["README.md"], conflict: null }), detail(against), detail(stale), detail(resolved), detail(conflictPair({ pairId: "ld", rev: 1 }))],
      { base: { name: "main", status: "unconfirmed" } },
    );
    const status = (id: string) => detailView(fleet, id).status;
    expect(status("ov")).toBe("overlap-only");
    expect(status("ag")).toBe("against-base");
    expect(status("ld"), "the detail has another rev").toBe("loading");
    expect(status("st")).toBe("stale");
    expect(status("ca")).toBe("calculating");
    expect(status("pe")).toBe("pending");
    expect(status("re")).toBe("resolved");
    expect(status("nope"), "an unknown pair").toBe("gone");
    expect(detailView(fleet, "ov").overlapFiles).toEqual(["README.md"]);
    expect(detailView(fleet, "pe").basePending?.base, "a pending base-unconfirmed pair carries the D11 warning").toBe("main");
    expect(detailView(fleet, "ag").title, "against the base: A is going to collide with {{base}}").toBe(`⚡ ${label(fleet, "feat-pagos")} is going to collide with main`);
  });
});

describe("the public module of src/conflicts (D6)", () => {
  it("the public module exports only the band model, the side ref, the view model and usePredictionView", () => {
    const source = readFileSync(fileURLToPath(new URL("./index.ts", import.meta.url)), "utf8");
    const exported = [...source.matchAll(/export\s+(?:type\s+)?\{([^}]*)\}/g)]
      .flatMap((m) => (m[1] ?? "").split(","))
      .map((name) => name.trim().replace(/^type\s+/, ""))
      .filter(Boolean)
      .sort();
    expect(exported, "the contract US-DSK-005 consumes").toEqual(["ConflictBandModel", "PredictionViewModel", "SideRef", "usePredictionView"]);
    expect(source, "no export * leaks the rest of the folder").not.toMatch(/export\s+\*/);
  });

  it("usePredictionView gives the same model the panel paints", () => {
    const p = conflictPair();
    const fleet = fleetWith([p], [detail(p)]);
    const { result } = renderHook(() => usePredictionView(fleet, fleetCards(fleet, NOW, true, t), NOW));
    expect(result.current.bands.map((b) => b.pairId), "a band for the conflict").toEqual([p.pairId]);
  });
});
