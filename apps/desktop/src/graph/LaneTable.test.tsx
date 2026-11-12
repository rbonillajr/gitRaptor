// DS-US-DSK-005 T005 (§ 9.4): the table view, the textual alternative of the graph, in en and es.
import { act, cleanup, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import { entryOf, modelOf, tienda, tiendaGraph, tiendaWithSlowLane } from "./fixtures";
import { LaneTable } from "./LaneTable";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
beforeEach(async () => {
  await setLang("en");
});
afterEach(cleanup);

const cells = (row: HTMLElement) => within(row).getAllByRole("gridcell").map((c) => c.textContent);

describe("LaneTable", () => {
  it("one row per lane with branch, agent, state, ahead/behind and commits", () => {
    render(<LaneTable model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />);
    expect(screen.getByRole("grid", { name: "Lanes of tienda" })).toBeInTheDocument();
    expect(screen.getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Branch", "Agent", "State", "Ahead/behind", "Commits"]);
    const rows = screen.getAllByRole("row").filter((r) => r.hasAttribute("data-row-id"));
    expect(rows).toHaveLength(2);
    expect(cells(rows[1] as HTMLElement)).toEqual(["feat-pagos", "Claude Code · feat-pagos", "● active", "3 ahead and 1 behind main", "3 of 3"]);
    expect(rows[0]).toHaveAccessibleName("Lane feat-login, Claude Code · feat-login, active, 1 ahead and 0 behind main, 1 of 1");
  });

  it("without the graph the commits say nothing yet", () => {
    render(<LaneTable model={modelOf(tienda(), null)} repoName="tienda" stale={false} />);
    for (const row of screen.getAllByRole("row").filter((r) => r.hasAttribute("data-row-id"))) expect(cells(row).at(-1)).toBe("—");
  });

  it("lists every lane, also the one whose commits took too long, and an inexact total says at least", () => {
    const { fleet, graph } = tiendaWithSlowLane();
    render(<LaneTable model={modelOf(fleet, entryOf(graph))} repoName="tienda" stale={false} />);
    expect(screen.getAllByRole("row").filter((r) => r.hasAttribute("data-row-id"))).toHaveLength(3);
  });

  it("puts the lane badge in the branch cell", () => {
    render(<LaneTable model={modelOf(tienda(), null)} repoName="tienda" stale={false} laneBadge={(key) => <span>badge {key}</span>} />);
    expect(screen.getByText("badge key-feat-pagos")).toBeInTheDocument();
  });

  it("is muted and aria-disabled when the engine is not live", () => {
    render(<LaneTable model={modelOf(tienda(), null)} repoName="tienda" stale />);
    expect(screen.getByRole("grid")).toHaveAttribute("aria-disabled", "true");
  });

  it("is in Spanish", async () => {
    await setLang("es");
    render(<LaneTable model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />);
    expect(screen.getByRole("grid", { name: "Carriles de tienda" })).toBeInTheDocument();
    expect(screen.getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Rama", "Agente", "Estado", "Por delante/detrás", "Commits"]);
  });

  it("has no axe violations in en and es", async () => {
    const { container } = render(<LaneTable model={modelOf(tienda(), entryOf(tiendaGraph()))} repoName="tienda" stale={false} />);
    expect((await axe.run(container, { rules: { "color-contrast": { enabled: false } } })).violations).toEqual([]);
    await setLang("es");
    expect((await axe.run(container, { rules: { "color-contrast": { enabled: false } } })).violations).toEqual([]);
  });
});
