// DS-US-DSK-005 T001 (§ 9.4, TS column of the graph): GraphView in en and es, with axe. Red until T005.
// The delta measuring and first-paint marks (T009) and the conflict badge and panel (T011) get their tests with those tasks.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { GraphDto, RepoFleet } from "@/client/engine-types";
import { setViewportWidth } from "../../tests/setup";
import { GraphView } from "./GraphView";
import { LOGIN, MIN, NOW, PAGOS, cardsOf, commit, commits, counted, entryOf, modelOf, ready, readyLane, session, tienda, tiendaDevelop, tiendaGraph, tiendaWithSlowLane, worktree } from "./fixtures";
import { useGraphPrefs } from "./graph-prefs";
import type { GraphEntry } from "./graph-types";

const ROW_PX = 44;

/** jsdom has no layout: every observed element reports this size, and canvas calls go nowhere. */
function stubLayout(size: { width: number; height: number }) {
  class FakeObserver {
    constructor(private readonly cb: ResizeObserverCallback) {}
    observe(target: Element) {
      this.cb([{ target, contentRect: { ...size, x: 0, y: 0, top: 0, left: 0, right: size.width, bottom: size.height, toJSON: () => ({}) } } as ResizeObserverEntry], this as unknown as ResizeObserver);
    }
    unobserve() {}
    disconnect() {}
  }
  vi.stubGlobal("ResizeObserver", FakeObserver);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(size.height);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(size.width);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
}

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));

function view(fleet: RepoFleet | undefined, entry: GraphEntry | null = null, props: { stale?: boolean } = {}) {
  return <GraphView model={modelOf(fleet, entry)} repoName="tienda" stale={props.stale ?? false} />;
}

/** Rows of the grid body, without its header row. */
const bodyRows = () => screen.getAllByRole("row").filter((r) => r.hasAttribute("data-row-id"));
const rowNamed = (name: RegExp | string) => screen.getByRole("row", { name });

beforeEach(async () => {
  setViewportWidth(1440);
  stubLayout({ width: 1152, height: 880 });
  useGraphPrefs.persist.clearStorage();
  await setLang("en");
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("GraphView lanes", () => {
  it("lanes show branch and agent name in en and es", async () => {
    const { rerender } = render(view(tienda(), entryOf(tiendaGraph())));
    expect(screen.getByRole("grid", { name: "Lane graph of tienda" })).toBeInTheDocument();
    const pagos = rowNamed(/^feat-pagos, commit/);
    expect(within(pagos).getByText("Claude Code · feat-pagos")).toBeInTheDocument();
    await setLang("es");
    rerender(view(tienda(), entryOf(tiendaGraph())));
    expect(screen.getByRole("grid", { name: "Grafo de carriles de tienda" })).toBeInTheDocument();
    expect(within(rowNamed(/^feat-pagos, commit/)).getByText("Claude Code · feat-pagos")).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "Rama / Etiqueta" })).toBeInTheDocument();
  });

  it("commits end in a fork row at their merge-base", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    const fork = rowNamed("feat-pagos branches off main at commit a1b2c3d");
    expect(within(fork).getByText("branches off main at a1b2c3d")).toBeInTheDocument();
    expect(bodyRows().indexOf(fork)).toBeGreaterThan(bodyRows().indexOf(rowNamed(/Scaffold the payments page/)));
  });

  it("wip rows sit above their lane head", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    const rows = bodyRows();
    const wip = rowNamed("feat-pagos, 2 uncommitted files");
    expect(within(wip).getByText("WIP · 2 uncommitted files")).toBeInTheDocument();
    expect(rows.indexOf(wip)).toBeLessThan(rows.indexOf(rowNamed(/Add the payment form/)));
    expect(rows.indexOf(rowNamed("main, 3 uncommitted files"))).toBeLessThan(rows.indexOf(wip));
  });

  it("a worktrees message updates the lane ahead count without reload", () => {
    const { rerender } = render(view(tienda()));
    expect(rowNamed(/^Lane feat-pagos/)).toHaveAccessibleName(expect.stringContaining("3 ahead and 1 behind main"));
    const next = tienda({
      worktrees: tienda().repo.worktrees.map((w) =>
        w.folder === "feat-pagos" ? { ...w, status: ready({ head: { kind: "branch", name: "feat-pagos" }, divergence: counted(4, 1) }) } : w,
      ),
    });
    rerender(view(next));
    expect(rowNamed(/^Lane feat-pagos/)).toHaveAccessibleName(expect.stringContaining("4 ahead and 1 behind main"));
  });

  it("a new commit appears in its lane without reload", () => {
    const { rerender } = render(view(tienda(), entryOf(tiendaGraph())));
    expect(screen.queryByText("Wire the gateway")).not.toBeInTheDocument();
    const next: GraphDto = tiendaGraph({
      lanes: [readyLane("feat-pagos", [commit(9, "Wire the gateway", NOW), ...PAGOS], { count: 4, exact: true }), readyLane("feat-login", LOGIN)],
    });
    rerender(view(tienda(), entryOf(next)));
    expect(screen.getByText("Wire the gateway")).toBeInTheDocument();
  });

  it("a commit row says you or another (unattributed) with its tooltip", async () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    const row = rowNamed(/^feat-pagos, commit \w{7}, Add the payment form/);
    expect(row).toHaveAccessibleName(expect.stringContaining("GitRaptor doesn't know yet which agent made this commit"));
    const cell = within(row).getByText("You or another (unattributed)");
    expect(cell).toHaveClass("text-text-muted");
    fireEvent.pointerEnter(cell);
    fireEvent.focus(cell);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("GitRaptor doesn't know yet which agent made this commit.");
  });

  it("base develop not found is announced and no lane is drawn", () => {
    render(view(tiendaDevelop(), entryOf(tiendaGraph())));
    expect(screen.getByText("Base branch develop not found: not computable")).toBeInTheDocument();
    expect(screen.getByText(/GitRaptor doesn't draw lanes against another branch/)).toBeInTheDocument();
    expect(screen.queryByRole("row", { name: /commit/ })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Base branch develop not found");
  });

  it("an unconfirmed base keeps the command out of the graph", () => {
    render(view(tienda({ repo: { base: { name: "main", status: "unconfirmed" } } })));
    expect(screen.getByText("Base main not confirmed: confirm the base branch to see the lanes")).toBeInTheDocument();
    expect(screen.queryByText(/raptor/)).not.toBeInTheDocument();
  });

  it("a lane and its card share name, colour slot and state", () => {
    const fleet = tienda();
    const cards = cardsOf(fleet);
    const model = modelOf(fleet, entryOf(tiendaGraph()));
    for (const lane of model.lanes) {
      const card = cards.find((c) => c.key === lane.key);
      expect(lane.agentLabel).toBe(card?.agent.label);
      expect(lane.colorSlot).toBe(card?.agent.colorSlot);
      expect(lane.stateText).toBe(card?.stateText);
    }
    render(<GraphView model={model} repoName="tienda" stale={false} />);
    const pagos = rowNamed(/^feat-pagos, commit \w{7}, Add the payment form/);
    expect(within(pagos).getByText("Claude Code · feat-pagos")).toBeInTheDocument();
  });

  it("an unsupported engine keeps level 1 with its note", () => {
    render(view(tienda(), entryOf(null, "unsupported")));
    expect(screen.getByText("This engine doesn't offer the graph commits; update or restart GitRaptor.")).toBeInTheDocument();
    expect(rowNamed(/^Lane feat-pagos/)).toBeInTheDocument();
  });

  it("a failed query keeps the last commits with its note", () => {
    render(view(tienda(), entryOf(tiendaGraph(), "failed")));
    expect(screen.getByText("Couldn't read the commits now; showing the last ones.")).toBeInTheDocument();
    expect(screen.getByText("Add the payment form")).toBeInTheDocument();
  });

  it("a summary is rendered as text", () => {
    const hostile = '<img src=x onerror="alert(1)"> ‮';
    const graph = tiendaGraph({ lanes: [readyLane("feat-pagos", [commit(1, hostile, NOW)], { count: 1, exact: true })] });
    const { container } = render(view(tienda(), entryOf(graph)));
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText(hostile, { exact: false })).toBeInTheDocument();
  });

  it("a too-slow lane keeps its summary row with the reason", () => {
    const { fleet, graph } = tiendaWithSlowLane();
    render(view(fleet, entryOf(graph)));
    expect(within(rowNamed(/^Lane feat-lento/)).getByText("took too long")).toBeInTheDocument();
  });
});

describe("GraphView column settings", () => {
  const openSettings = () => fireEvent.click(screen.getByRole("button", { name: "Column settings" }));

  it("hiding sha and author removes their columns and keeps message", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    expect(screen.getByRole("columnheader", { name: "Sha" })).toBeInTheDocument();
    openSettings();
    fireEvent.click(screen.getByRole("switch", { name: "Sha" }));
    fireEvent.click(screen.getByRole("switch", { name: "Author" }));
    expect(screen.queryByRole("columnheader", { name: "Sha" })).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "Author" })).not.toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "Commit message" })).toBeInTheDocument();
    expect(useGraphPrefs.getState().columns).toMatchObject({ sha: false, author: false, date: true });
    expect(screen.getByText("Always visible")).toBeInTheDocument();
  });

  it("reset brings every column back", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    openSettings();
    fireEvent.click(screen.getByRole("switch", { name: "Date / Time" }));
    expect(screen.queryByRole("columnheader", { name: "Date / Time" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    for (const name of ["Branch / Tag", "Graph", "Commit message", "Author", "Date / Time", "Sha"]) {
      expect(screen.getByRole("columnheader", { name })).toBeInTheDocument();
    }
  });

  it("a folded column says it does not fit and keeps its switch as the person left it", () => {
    stubLayout({ width: 788, height: 880 });
    render(view(tienda(), entryOf(tiendaGraph())));
    expect(screen.queryByRole("columnheader", { name: "Sha" })).not.toBeInTheDocument();
    openSettings();
    expect(screen.getByRole("switch", { name: "Sha" })).toBeChecked();
    expect(screen.getAllByText("Doesn't fit at this width").length).toBeGreaterThanOrEqual(2);
  });
});

describe("GraphView accessibility", () => {
  it("the agent name is main text and the lane colour is a hidden dot", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    const name = within(rowNamed(/^feat-pagos, commit \w{7}, Add the payment form/)).getByText("Claude Code · feat-pagos");
    expect(name).toHaveClass("text-text-main");
    expect([...name.classList].some((c) => c.startsWith("text-lane-"))).toBe(false);
    const dot = name.previousElementSibling;
    expect(dot).toHaveAttribute("aria-hidden", "true");
    expect([...(dot?.classList ?? [])].some((c) => c.startsWith("bg-lane-"))).toBe(true);
  });

  it("every lane has its agent name and state symbol next to its colour", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    for (const folder of ["feat-pagos", "feat-login"]) {
      const head = bodyRows().find((r) => r.getAttribute("aria-label")?.startsWith(`${folder}, commit`));
      expect(head).toBeDefined();
      expect(head).toHaveTextContent(`Claude Code · ${folder}`);
      expect(head).toHaveTextContent("●");
    }
  });

  it("each row has a full sentence as its name", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    const names = bodyRows().map((r) => r.getAttribute("aria-label"));
    expect(names).toContain("main, 3 uncommitted files");
    expect(names).toContain("Base branch main");
    const sentences = names.filter((n) => n !== null && (n.includes("uncommitted") || n.includes(", commit")));
    expect(sentences.length).toBeGreaterThan(0);
    expect(sentences.every((n) => n !== null && n.includes(", "))).toBe(true);
    expect(names.some((n) => n === "Base branch main")).toBe(true);
  });

  it("the canvas is hidden from assistive tech", () => {
    const { container } = render(view(tienda(), entryOf(tiendaGraph())));
    const canvases = container.getElementsByTagName("canvas");
    expect(canvases.length).toBe(1);
    expect(canvases[0]?.closest("[aria-hidden='true']")).not.toBeNull();
  });

  it("has no axe violations in en and es", async () => {
    const { container, rerender } = render(view(tienda(), entryOf(tiendaGraph())));
    expect((await axe.run(container)).violations).toEqual([]);
    await setLang("es");
    rerender(view(tienda(), entryOf(tiendaGraph())));
    expect((await axe.run(container)).violations).toEqual([]);
  });

  it("arrows, page and home/end move through every row", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    const rows = bodyRows();
    rows[0]?.focus();
    fireEvent.keyDown(rows[0] as HTMLElement, { key: "ArrowDown" });
    expect(bodyRows()[1]).toHaveFocus();
    fireEvent.keyDown(bodyRows()[1] as HTMLElement, { key: "End" });
    expect(bodyRows().at(-1)).toHaveFocus();
    fireEvent.keyDown(bodyRows().at(-1) as HTMLElement, { key: "Home" });
    expect(bodyRows()[0]).toHaveFocus();
    fireEvent.keyDown(bodyRows()[0] as HTMLElement, { key: "PageDown" });
    expect(bodyRows()[Math.min(10, bodyRows().length - 1)]).toHaveFocus();
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "PageUp" });
    expect(bodyRows()[0]).toHaveFocus();
  });

  it("option-command-t announces the view and keeps focus", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Macintosh");
    render(view(tienda(), entryOf(tiendaGraph())));
    const first = bodyRows()[0] as HTMLElement;
    first.focus();
    fireEvent.keyDown(document.body, { key: "t", code: "KeyT", metaKey: true, altKey: true });
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent("Table view");
    expect(screen.getByRole("grid", { name: "Lanes of tienda" })).toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: "t", code: "KeyT", metaKey: true, altKey: true });
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent("Graph view");
    expect(useGraphPrefs.getState().view).toBe("graph");
  });

  it("the view control switches between graph and table", () => {
    render(view(tienda(), entryOf(tiendaGraph())));
    fireEvent.click(screen.getByRole("radio", { name: "Table" }));
    expect(screen.getByRole("grid", { name: "Lanes of tienda" })).toBeInTheDocument();
    expect(useGraphPrefs.getState().view).toBe("table");
  });
});

describe("GraphView virtualisation and clipping", () => {
  const many = () => {
    const names = Array.from({ length: 12 }, (_, i) => `lane-${String(i).padStart(2, "0")}`);
    const fleet = tienda({
      worktrees: [
        worktree("tienda", { main: true, path: "/w/tienda", status: ready() }),
        ...names.map((name) => worktree(name, { status: ready({ head: { kind: "branch", name }, divergence: counted(50, 0) }) })),
      ],
      sessions: names.map((n) => session(`s-${n}`, n)),
    });
    const graph = tiendaGraph({ lanes: names.map((name, i) => readyLane(name, commits(50, name, NOW - i * MIN, 100 + i * 100), { count: 50, exact: true })), lanesTotal: 12 });
    return { fleet, graph };
  };

  it("the dom never holds more than twice the visible rows", () => {
    stubLayout({ width: 1152, height: 10 * ROW_PX });
    const { fleet, graph } = many();
    render(view(fleet, entryOf(graph)));
    expect(bodyRows().length).toBeGreaterThan(0);
    expect(bodyRows().length).toBeLessThanOrEqual(20);
    expect(screen.getByRole("grid")).toHaveAttribute("aria-rowcount", String(modelOf(fleet, entryOf(graph)).rows.length + 1));
  });

  it("lanes that do not fit say so and point to the table view", () => {
    const names = Array.from({ length: 30 }, (_, i) => `lane-${String(i).padStart(2, "0")}`);
    const fleet = tienda({
      worktrees: [worktree("tienda", { main: true, path: "/w/tienda", status: ready() }), ...names.map((name) => worktree(name, { status: ready({ head: { kind: "branch", name }, divergence: counted(1, 0) }) }))],
      sessions: [],
    });
    const graph = tiendaGraph({ lanes: names.map((name, i) => readyLane(name, commits(1, name, NOW - i * MIN, 100 + i))), lanesTotal: 30 });
    render(view(fleet, entryOf(graph)));
    expect(screen.getByText("The drawing shows 20 of 30 lanes; the table view lists them all.")).toBeInTheDocument();
  });
});
