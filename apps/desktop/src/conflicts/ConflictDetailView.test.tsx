// D8-DETAILVIEW (DS-US-DSK-008 D7, D18, D19, § 9.4): the detail of a pair in the centre: files with the lines of each
// side, paths as text, never a detail of another rev, the focus on the heading when it opens, and Esc by layers.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n, { type TFunction } from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { PairDetailDto, PairDto, RepoFleet } from "@/client/engine-types";
import { fleetCards } from "@/fleet/fleet-model";
import { NOW, pair, tienda } from "@/fleet/fixtures";
import { useUiStore } from "@/state/ui-store";
import { ConflictDetailView } from "./ConflictDetailView";
import { ConflictPanel } from "./ConflictPanel";

const t = i18n.t.bind(i18n) as unknown as TFunction;
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const A = "Claude Code · feat-pagos";
const B = "Claude Code · feat-login";
const CONFLICT: PairDto = pair("feat-pagos", "feat-login", {
  pairId: "c1",
  rev: 4,
  state: "current",
  level: "conflict",
  computedAtUtcMs: NOW - 40_000,
  conflictFiles: 1,
});
const detailOf = (p: PairDto, path = "pagos.rs", over: Partial<PairDetailDto> = {}): PairDetailDto => ({
  pair: p,
  overlap: [],
  conflict: { files: [{ path, kind: "content", ours: [{ start: 40, len: 19 }], theirs: [{ start: 44, len: 18 }], hunksTruncated: false, hasMergeRule: false }], truncated: false },
  ...over,
});
const fleetWith = (pairs: PairDto[], details: PairDetailDto[] = []): RepoFleet => ({
  ...tienda({ pairs, repo: { limits: ["local-only"] } }),
  details: new Map(details.map((d) => [d.pair.pairId, d])),
});
const cardsOf = (fleet: RepoFleet) => fleetCards(fleet, NOW, true, t);
const detailView = (fleet: RepoFleet, pairId = "c1") => <ConflictDetailView fleet={fleet} cards={cardsOf(fleet)} pairId={pairId} nowMs={NOW} />;
const show = (fleet: RepoFleet, pairId = "c1") => render(detailView(fleet, pairId));

/** The centre as the shell paints it: the panel and, when the store says so, the detail. */
function Centre({ fleet }: { fleet: RepoFleet }) {
  const center = useUiStore((s) => s.center);
  return (
    <div>
      <ConflictPanel fleet={fleet} cards={cardsOf(fleet)} nowMs={NOW} />
      {center.view === "pair" ? detailView(fleet, center.pairId) : null}
    </div>
  );
}
const esc = () => fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape", code: "Escape" });

beforeEach(async () => {
  useUiStore.setState({ selectedRepoId: "tienda", center: { view: "pair", repoId: "tienda", pairId: "c1" } });
  await setLang("en");
});
afterEach(cleanup);

describe("ConflictDetailView", () => {
  it("lists pagos.rs with the lines of each side", () => {
    show(fleetWith([CONFLICT], [detailOf(CONFLICT)]));
    const heading = screen.getByRole("heading", { level: 2, name: /collide/ });
    expect(heading.textContent).toBe(`⚡ ${A} ↔ ${B} are going to collide`);
    expect(heading.querySelector('[aria-hidden="true"]')?.textContent, "D19: the glyph is hidden from assistive technology").toBe("⚡");
    expect(document.activeElement, "D7: the focus goes to the heading when the detail opens").toBe(heading);
    expect(screen.getByText("Predicted conflict (committed work)")).toBeInTheDocument();
    const files = screen.getAllByRole("listitem");
    const file = files.find((li) => li.textContent?.includes("pagos.rs"));
    expect(file, "a list item for pagos.rs").toBeDefined();
    expect(within(file as HTMLElement).getByText("pagos.rs").tagName).toBe("CODE");
    expect(file?.textContent).toContain(`${A} — lines 40–58`);
    expect(file?.textContent).toContain(`${B} — lines 44–61`);
    expect(screen.getByText("feat-pagos ↔ feat-login · computed 40 s ago"), "folder A ↔ folder B and the age").toBeInTheDocument();
    expect(screen.getByText("What to do")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to the fleet" })).toBeInTheDocument();
  });

  it("the detail has no write buttons and no copy-notice button", () => {
    show(fleetWith([CONFLICT], [detailOf(CONFLICT)]));
    const names = screen.getAllByRole("button").map((b) => b.textContent);
    expect(screen.getByRole("heading", { level: 2, name: /collide/ }), "painted").toBeInTheDocument();
    expect(names.sort(), "only back and the limits popover").toEqual(["Back to the fleet", "Limits (?)"]);
  });

  it("a detail of another rev is not shown", () => {
    const stale = detailOf({ ...CONFLICT, rev: 3 });
    show(fleetWith([CONFLICT], [stale]));
    expect(screen.getByText("Loading the files…"), "D4: the view says it is loading, it does not show the old files").toBeInTheDocument();
    expect(screen.queryByText("pagos.rs")).toBeNull();
  });

  it("paths are rendered as text", () => {
    const hostile = "<img src=x onerror=alert(1)>\\x1b]52;c;QUFB.rs";
    const { container } = show(fleetWith([CONFLICT], [detailOf(CONFLICT, hostile)]));
    expect(screen.getByText(hostile).tagName, "the path is the text of a <code>").toBe("CODE");
    expect(container.querySelector("img"), "never markup").toBeNull();
  });

  it("calculating, resolved and gone never say there is a conflict, and offer the way back", () => {
    const calc = { ...CONFLICT, state: "calculating" as const, level: null, computedAtUtcMs: null };
    const { unmount } = show(fleetWith([calc]));
    expect(screen.getByText("Calculating…")).toBeInTheDocument();
    unmount();
    const resolved = { ...CONFLICT, level: "none" as const };
    const view = show(fleetWith([resolved], [detailOf(resolved)]));
    expect(view.container.textContent).toContain(`No conflict predicted between ${A} and ${B} anymore (40 s ago).`);
    expect(screen.getByRole("button", { name: "Back to the fleet" })).toBeInTheDocument();
    view.unmount();
    show(fleetWith([CONFLICT]), "missing");
    expect(screen.getByText("This pair is no longer predicted.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back to the fleet" })).toBeInTheDocument();
  });

  it("Esc goes back and returns focus to the band", () => {
    const fleet = fleetWith([CONFLICT], [detailOf(CONFLICT)]);
    useUiStore.setState({ center: { view: "main" } });
    render(<Centre fleet={fleet} />);
    const open = screen.getByRole("button", { name: /^View the conflict between/ });
    fireEvent.click(open);
    expect(useUiStore.getState().center, "View opens the pair").toEqual({ view: "pair", repoId: "tienda", pairId: "c1" });
    expect(document.activeElement, "the heading has the focus").toBe(screen.getByRole("heading", { level: 2, name: /collide/ }));
    esc();
    expect(useUiStore.getState().center, "Esc goes back to the main view").toEqual({ view: "main" });
    expect(document.activeElement, "the focus returns to the View button of the band").toBe(open);
  });

  it("Esc closes the open popover first, then the detail", () => {
    show(fleetWith([CONFLICT], [detailOf(CONFLICT)]));
    fireEvent.click(screen.getByRole("button", { name: "Limits (?)" }));
    expect(screen.getByRole("dialog", { name: "Prediction limits" })).toBeInTheDocument();
    esc();
    expect(screen.queryByRole("dialog"), "the first Esc closes the popover").toBeNull();
    expect(useUiStore.getState().center.view, "and the detail stays").toBe("pair");
    esc();
    expect(useUiStore.getState().center, "the second Esc closes the detail").toEqual({ view: "main" });
  });

  it.each(["en", "es"] as const)("has no axe violations in %s", async (lang) => {
    await setLang(lang);
    const { container } = show(fleetWith([CONFLICT], [detailOf(CONFLICT)]));
    expect(screen.getByRole("heading", { level: 2, name: /collide|chocar/ }), "painted before axe").toBeInTheDocument();
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
