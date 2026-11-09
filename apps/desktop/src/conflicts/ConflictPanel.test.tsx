// D8-PANEL (DS-US-DSK-008 D5, D11, D20, § 9.4): the predicted-conflict panel of the centre, in en and es, with axe
// at 1440 and 1200 px. It takes the fleet, the cards and the clock; it reads the open action from the ui store.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n, { type TFunction } from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { ConfirmCommandDto, PairDetailDto, PairDto, RepoFleet } from "@/client/engine-types";
import { fleetCards } from "@/fleet/fleet-model";
import { NOW, pair, tienda } from "@/fleet/fixtures";
import { useUiStore } from "@/state/ui-store";
import { setViewportWidth } from "../../tests/setup";
import { ConflictPanel } from "./ConflictPanel";

const t = i18n.t.bind(i18n) as unknown as TFunction;
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const AGO = 40_000;
const conflict = (id: string, over: Partial<PairDto> = {}): PairDto =>
  pair("feat-pagos", "feat-login", { pairId: id, rev: 4, state: "current", level: "conflict", computedAtUtcMs: NOW - AGO, conflictFiles: 1, ...over });
const baseOverlap = (over: Partial<PairDto> = {}): PairDto =>
  pair("feat-pagos", "base", { pairId: "against-base", rev: 4, state: "current", level: "overlap", computedAtUtcMs: NOW - AGO, overlapFiles: 1, ...over });
const pendingBase = () => baseOverlap({ state: "pending", level: null, reason: "base-unconfirmed", computedAtUtcMs: null, overlapFiles: 0 });
const detailOf = (p: PairDto, path = "pagos.rs"): PairDetailDto => ({
  pair: p,
  overlap: [],
  conflict: { files: [{ path, kind: "content", ours: [{ start: 40, len: 19 }], theirs: [{ start: 44, len: 18 }], hunksTruncated: false, hasMergeRule: false }], truncated: false },
});
const OFFERED: ConfirmCommandDto = { state: "offered", command: "'/opt/gitraptor/raptor' 'guard' 'confirm' '/w/tienda/.git'", withPath: true };
const fleetWith = (pairs: PairDto[], over: Partial<RepoFleet["repo"]> = {}, details: PairDetailDto[] = []): RepoFleet => {
  const fleet = tienda({ pairs, repo: { limits: ["overlap-includes-uncommitted", "local-only"], confirmCommand: OFFERED, ...over } });
  return { ...fleet, details: new Map(details.map((d) => [d.pair.pairId, d])) };
};
const show = (fleet: RepoFleet) => render(<ConflictPanel fleet={fleet} cards={fleetCards(fleet, NOW, true, t)} nowMs={NOW} />);
const panel = () => screen.getByRole("region", { name: "Predicted conflicts" });

let writeText: ReturnType<typeof vi.fn>;
beforeEach(async () => {
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  setViewportWidth(1440);
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "main" } });
  await setLang("en");
});
afterEach(cleanup);

describe("ConflictPanel", () => {
  it("shows a predicted conflict between feat-pagos and feat-login with pagos.rs", () => {
    const p = conflict("c1");
    show(fleetWith([p], {}, [detailOf(p)]));
    const list = within(panel()).getByRole("list", { name: "Predicted conflicts in tienda" });
    const items = within(list).getAllByRole("listitem");
    expect(items, "one band for the conflict").toHaveLength(1);
    expect(items[0]?.textContent).toContain("feat-pagos");
    expect(items[0]?.textContent).toContain("feat-login");
    expect(within(items[0] as HTMLElement).getByText("pagos.rs").tagName).toBe("CODE");
    expect(items[0]?.textContent, "the age of the result").toContain("40 s ago");
    fireEvent.click(within(items[0] as HTMLElement).getByRole("button", { name: /^View the conflict between/ }));
    expect(useUiStore.getState().center, "View opens the pair in the centre").toEqual({ view: "pair", repoId: "tienda", pairId: "c1" });
  });

  it("a pending base says so, offers the confirm command and keeps the worktree conflict", () => {
    const c = conflict("c1");
    show(fleetWith([c, pendingBase()], { base: { name: "main", status: "unconfirmed" } }, [detailOf(c)]));
    expect(within(panel()).getByText("Base branch waiting for confirmation (main)")).toBeInTheDocument();
    expect(within(panel()).getByText(OFFERED.state === "offered" ? OFFERED.command : "").tagName, "the command is code").toBe("CODE");
    expect(within(panel()).getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(panel().textContent, "the hint says to run it in the user's own terminal").toContain("Run it in your own terminal (not an agent's).");
    expect(within(panel()).getAllByRole("listitem"), "the ⚡ between worktrees stays in view").toHaveLength(1);
    expect(panel().textContent).toMatch(/Pending: .+ \(1 pair\)/);
  });

  it("copying the command writes exactly the command of the backend", async () => {
    show(fleetWith([pendingBase()], { base: { name: "main", status: "unconfirmed" } }));
    await act(async () => void fireEvent.click(within(panel()).getByRole("button", { name: "Copy" })));
    expect(writeText).toHaveBeenCalledExactlyOnceWith(OFFERED.state === "offered" ? OFFERED.command : "");
  });

  it("a base pair that leaves pending hides the warning even if the snapshot still says unconfirmed", () => {
    show(fleetWith([baseOverlap()], { base: { name: "main", status: "unconfirmed" } }, [detailOf(baseOverlap())]));
    expect(panel(), "the panel is painted for the warning pair").toBeInTheDocument();
    expect(screen.queryByText(/Base branch waiting for confirmation/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it("without a path the hint names the repo folder", () => {
    const withoutPath: ConfirmCommandDto = { state: "offered", command: "'/opt/gitraptor/raptor' 'guard' 'confirm'", withPath: false };
    show(fleetWith([pendingBase()], { base: { name: "main", status: "unconfirmed" }, confirmCommand: withoutPath }));
    expect(panel().textContent).toContain("Run it in your own terminal, in the folder of tienda.");
    expect(panel().textContent, "the hint with a path is not the one shown").not.toContain("Run it in your own terminal (not an agent's).");
  });

  it("raptor unknown shows why and no copy button", () => {
    show(fleetWith([pendingBase()], { base: { name: "main", status: "unconfirmed" }, confirmCommand: { state: "not-offered", reason: "raptor-unknown" } }));
    expect(panel().textContent).toContain("GitRaptor can't find its own `raptor` to write the command.");
    expect(within(panel()).queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it("shows at most 3 bands and expands the rest", () => {
    const pairs = ["c1", "c2", "c3", "c4", "c5"].map((id) => conflict(id, { b: `key-w-${id}` }));
    show(fleetWith(pairs));
    expect(within(panel()).getAllByRole("listitem"), "D20: three bands").toHaveLength(3);
    const more = within(panel()).getByRole("button", { name: "and 2 more conflicts" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(more).toHaveAttribute("aria-controls");
    fireEvent.click(more);
    expect(within(panel()).getAllByRole("listitem")).toHaveLength(5);
    const less = within(panel()).getByRole("button", { name: "Show fewer" });
    expect(less).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(less);
    expect(within(panel()).getAllByRole("listitem")).toHaveLength(3);
  });

  it("freshness lines are polite status text and the limits open from a popover", () => {
    const calculating = baseOverlap({ pairId: "calc", state: "calculating", level: null, computedAtUtcMs: null });
    show(fleetWith([conflict("c1"), calculating]));
    const status = within(panel()).getAllByRole("status").find((n) => n.textContent === "Calculating 1 of 2 pairs");
    expect(status, "the calculating line is a <p role=status>").toBeDefined();
    fireEvent.click(within(panel()).getByRole("button", { name: "Limits (?)" }));
    const dialog = screen.getByRole("dialog", { name: "Prediction limits" });
    expect(within(dialog).getAllByRole("listitem"), "one text per limit").toHaveLength(2);
    expect(dialog.textContent).not.toContain("conflicts.limit.");
  });

  it("with no pairs only the limits line shows", () => {
    show(fleetWith([]));
    expect(within(panel()).queryByRole("list", { name: /Predicted conflicts in/ })).toBeNull();
    expect(panel().textContent).toContain("Uncommitted work counts as overlap; predicted conflicts only use committed work.");
    expect(within(panel()).getByRole("button", { name: "Limits (?)" })).toBeInTheDocument();
  });

  it("es: the panel speaks Spanish", async () => {
    await setLang("es");
    const p = conflict("c1");
    show(fleetWith([p], {}, [detailOf(p)]));
    expect(screen.getByRole("region", { name: "Conflictos previstos" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Conflictos previstos en tienda" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Ver el conflicto entre/ })).toBeInTheDocument();
  });

  it.each([
    ["en", 1440],
    ["es", 1440],
    ["en", 1200],
    ["es", 1200],
  ] as const)("has no axe violations in %s at %i px", async (lang, width) => {
    setViewportWidth(width);
    await setLang(lang);
    const c = conflict("c1");
    const { container } = show(fleetWith([c, pendingBase()], { base: { name: "main", status: "unconfirmed" } }, [detailOf(c)]));
    expect(within(container).getAllByRole("listitem").length, "the panel is painted before axe looks at it").toBeGreaterThan(0);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
