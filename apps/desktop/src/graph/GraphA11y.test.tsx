// DS-US-DSK-005 T005 acceptance: axe-core finds no violations at 1440 and 1200 px, in en and es, in the graph view, the
// table view and with Column settings open. In jsdom there is no layout: the width of the center for each window width
// is the one of D11 (1440 with the detail open: 788; 1200 with the drawer closed: 912).
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { setViewportWidth } from "../../tests/setup";
import { GraphView } from "./GraphView";
import { entryOf, modelOf, tienda, tiendaGraph } from "./fixtures";
import { useGraphPrefs } from "./graph-prefs";

function stubLayout(width: number) {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(width);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(880);
}

beforeEach(() => {
  useGraphPrefs.setState({ view: "graph" });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const CASES = [
  { window: 1440, center: 788 },
  { window: 1200, center: 912 },
] as const;

describe("graph axe", () => {
  for (const { window: viewport, center } of CASES) {
    for (const lang of ["en", "es"] as const) {
      for (const view of ["graph", "table"] as const) {
        it(`has no violations at ${viewport} px in ${lang}, ${view} view`, async () => {
          setViewportWidth(viewport);
          stubLayout(center);
          await act(async () => void (await i18n.changeLanguage(lang)));
          useGraphPrefs.setState({ view });
          const { container } = render(<GraphView model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />);
          const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
          expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
        });
      }
    }
  }

  it("has no violations with Column settings open", async () => {
    stubLayout(788);
    await act(async () => void (await i18n.changeLanguage("en")));
    render(<GraphView model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Column settings" }));
    // The popover lives in a portal outside the test container; the page-level landmark rule is the app shell's.
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false }, region: { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });

  it("has no violations when the base is missing, and when the engine is not live", async () => {
    stubLayout(788);
    await act(async () => void (await i18n.changeLanguage("en")));
    const { container } = render(<GraphView model={modelOf(tienda({ repo: { base: { name: "main", status: "unconfirmed" } } }))} repoName="tienda" stale />);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
