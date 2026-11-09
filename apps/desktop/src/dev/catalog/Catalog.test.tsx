/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-CATALOG (TS-DSK-003 T009, D5-D7, D16): the dev catalog paints every primitive in a dark and a light panel
// at once, with a reduced-motion switch and a language switch; axe finds nothing in either panel.
// Assumption: the language SegmentedControl options are labelled "en"/"es" or "English"/"Español" (Dev Spec does not fix them).
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import { Catalog, CATALOG_SENTINEL } from "./Catalog";

const dark = () => screen.getByRole("region", { name: "Dark theme" });
const light = () => screen.getByRole("region", { name: "Light theme" });

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-CATALOG dev catalog", () => {
  it("TS003-CATALOG: exports the sentinel and marks its root with it", () => {
    const { container } = render(<Catalog />);
    expect(CATALOG_SENTINEL).toBe("gr-dev-catalog");
    expect(container.querySelector(`[data-gr-dev-catalog="${CATALOG_SENTINEL}"]`), "root carries data-gr-dev-catalog").not.toBeNull();
  });

  it("TS003-CATALOG: a dark and a light panel exist side by side with their bounded data-theme", () => {
    render(<Catalog />);
    expect(dark()).toHaveAttribute("data-theme", "dark");
    expect(light()).toHaveAttribute("data-theme", "light");
  });

  it("TS003-CATALOG: each panel paints the primitives (one section per primitive with its heading)", () => {
    render(<Catalog />);
    for (const panel of [dark(), light()]) {
      for (const name of ["Button", "IconButton", "Chip", "Pill", "StatusPill", "StatePanel", "CopyCommand", "Skeleton", "StatusDot", "RepoBadge", "SegmentedControl", "Toggle", "Tree", "Menu", "Popover", "Toast", "Dialog", "Tooltip", "Table"]) {
        expect(within(panel).getByRole("heading", { name }), `${name} heading in ${panel.getAttribute("data-theme")}`).toBeInTheDocument();
      }
    }
  });

  it("TS003-CATALOG: the reduced-motion switch sets data-gr-motion=reduce on the root", () => {
    const { container } = render(<Catalog />);
    const root = container.querySelector("[data-gr-dev-catalog]")!;
    expect(root.hasAttribute("data-gr-motion"), "motion on by default").toBe(false);
    fireEvent.click(screen.getByRole("switch", { name: "Reduced motion" }));
    expect(root, "the switch reduces motion").toHaveAttribute("data-gr-motion", "reduce");
  });

  it("TS003-CATALOG: switching to es turns Recoverable into Recuperable in both panels", () => {
    render(<Catalog />);
    expect(within(dark()).getAllByText("Recoverable").length, "en Pill label in dark").toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("radio", { name: /^(es|español|spanish)$/i }));
    expect(within(dark()).getAllByText("Recuperable").length, "es Pill label in dark").toBeGreaterThan(0);
    expect(within(light()).getAllByText("Recuperable").length, "es Pill label in light").toBeGreaterThan(0);
    expect(within(dark()).queryAllByText("Recoverable")).toHaveLength(0);
  });

  it.each(["dark", "light"] as const)("TS003-CATALOG: no axe violations in the %s panel", async (theme) => {
    render(<Catalog />);
    const panel = theme === "dark" ? dark() : light();
    const results = await axe.run(panel, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
