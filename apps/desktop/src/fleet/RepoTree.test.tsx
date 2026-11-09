/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// DS-US-DSK-004 T005 (§ 9.4, TS column that needs no IPC): the repo tree with its "Worktrees" group.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { RepoSummaryDto } from "@/client/engine-types";
import { setViewportWidth } from "../../tests/setup";
import { fleetCards } from "./fleet-model";
import { NOW, session, tienda, worktree } from "./fixtures";
import { RepoTree } from "./RepoTree";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const repo = (repoId: string, over: Partial<RepoSummaryDto> = {}): RepoSummaryDto => ({
  repoId,
  name: repoId,
  path: `/w/${repoId}`,
  state: "observed",
  tier: "active",
  attention: { conflicts: { state: "counted", count: 0 }, denials: { state: "counted", count: 0 } },
  ...over,
});
const REPOS = [repo("notas"), repo("tienda")];
const cards = () => fleetCards(tienda(), NOW, true, i18n.getFixedT("en"));

function Harness({ repos = REPOS, initial = null, onOpen = () => {} }: { repos?: RepoSummaryDto[]; initial?: string | null; onOpen?: (id: string) => void }) {
  const [selected, setSelected] = useState<string | null>(initial);
  return (
    <RepoTree
      repos={repos}
      selectedRepoId={selected}
      cards={selected === "tienda" ? cards() : []}
      onOpen={(id) => {
        setSelected(id);
        onOpen(id);
      }}
    />
  );
}
/** The main worktree has the repo's name, so rows are told apart by level: 1 repo, 2 group, 3 worktree. */
const row = (name: string, level = 1) => {
  const hit = screen.getAllByRole("treeitem", { name }).find((r) => r.getAttribute("aria-level") === String(level));
  if (!hit) throw new Error(`no treeitem "${name}" at level ${level}`);
  return hit;
};
const press = (key: string) => fireEvent.keyDown(document.activeElement!, { key });

beforeEach(async () => {
  setViewportWidth(1440);
  await setLang("en");
});
afterEach(cleanup);

describe("RepoTree", () => {
  it("is a tree headed Repos with one row per repo", () => {
    render(<Harness />);
    expect(screen.getByRole("heading", { name: "Repos" })).toBeInTheDocument();
    const tree = screen.getByRole("tree", { name: "Repos" });
    expect(within(tree).getAllByRole("treeitem").map((r) => r.getAttribute("aria-label"))).toEqual(["notas", "tienda"]);
  });

  it("choosing a repo opens it and lists its worktrees", () => {
    const onOpen = vi.fn();
    render(<Harness onOpen={onOpen} />);
    expect(screen.queryByRole("treeitem", { name: "Worktrees" })).toBeNull();
    fireEvent.click(row("tienda"));
    expect(onOpen).toHaveBeenCalledExactlyOnceWith("tienda");
    expect(row("tienda")).toHaveAttribute("aria-selected", "true");
    expect(row("tienda")).toHaveAttribute("aria-expanded", "true");
    expect(row("Worktrees", 2)).toHaveAttribute("aria-expanded", "true");
    expect(screen.getAllByRole("treeitem").map((r) => r.getAttribute("aria-label"))).toEqual([
      "notas",
      "tienda",
      "Worktrees",
      "tienda",
      "feat-login, active",
      "feat-pagos, active",
    ]);
  });

  it("Enter on a repo opens it", () => {
    const onOpen = vi.fn();
    render(<Harness onOpen={onOpen} />);
    act(() => row("notas").focus());
    press("Enter");
    expect(onOpen).toHaveBeenCalledExactlyOnceWith("notas");
  });

  it("does not open the repo that is already open again", () => {
    const onOpen = vi.fn();
    render(<Harness initial="tienda" onOpen={onOpen} />);
    fireEvent.click(row("tienda"));
    act(() => row("tienda").focus());
    press("Enter");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("moves with the arrow keys, expands and collapses", () => {
    render(<Harness initial="tienda" />);
    act(() => row("notas").focus());
    press("ArrowDown");
    expect(document.activeElement).toBe(row("tienda"));
    press("ArrowLeft");
    expect(row("tienda")).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("treeitem", { name: "Worktrees" })).toBeNull();
    press("ArrowRight");
    expect(row("tienda")).toHaveAttribute("aria-expanded", "true");
    press("ArrowRight");
    expect(document.activeElement).toBe(row("Worktrees", 2));
    press("End");
    expect(document.activeElement).toBe(row("feat-pagos, active", 3));
    press("Home");
    expect(document.activeElement).toBe(row("notas"));
  });

  it("the worktrees follow the fleet order with a status dot for active agents", () => {
    render(<Harness initial="tienda" />);
    const items = screen.getAllByRole("treeitem").filter((r) => r.getAttribute("aria-level") === "3");
    expect(items.map((r) => r.textContent)).toEqual(["tienda", "feat-login", "feat-pagos"]);
    expect(items[0]?.querySelector("span[aria-hidden=true].rounded-pill")).toBeNull();
    expect(items[1]?.querySelector("span[aria-hidden=true].rounded-pill")).not.toBeNull();
  });

  it("an inactive agent gets the hollow dot and its state in the name", () => {
    const fleet = tienda({ sessions: [session("claude-1", "feat-pagos", { state: "inactive" })], worktrees: [worktree("tienda", { main: true }), worktree("feat-pagos")] });
    const inactive = fleetCards(fleet, NOW, true, i18n.getFixedT("en"));
    render(<RepoTree repos={REPOS} selectedRepoId="tienda" cards={inactive} onOpen={() => {}} />);
    expect(row("feat-pagos, inactive", 3)).toBeInTheDocument();
  });

  it("shows the predicted conflicts badge with its label", () => {
    const repos = [repo("tienda", { attention: { conflicts: { state: "counted", count: 3 }, denials: { state: "counted", count: 0 } } }), repo("notas")];
    render(<Harness repos={repos} />);
    expect(row("tienda, 3 predicted conflicts")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "3 predicted conflicts" })).toHaveTextContent("⚡3");
    expect(row("notas")).not.toHaveTextContent("⚡");
  });

  it("no badge for zero or unavailable counts", () => {
    const unavailable = { conflicts: { state: "unavailable" }, denials: { state: "unavailable" } } as const;
    render(<Harness repos={[repo("a", { attention: unavailable }), repo("b")]} />);
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("an unavailable repo says so, is disabled and cannot be opened", () => {
    const onOpen = vi.fn();
    render(<Harness repos={[repo("gone", { state: "unavailable", tier: null }), repo("tienda")]} onOpen={onOpen} />);
    const gone = row("gone, unavailable");
    expect(gone).toHaveAttribute("aria-disabled", "true");
    expect(gone).toHaveTextContent("unavailable");
    fireEvent.click(gone);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("a closed repo returns to the tree: no selection, no worktrees", () => {
    const { rerender } = render(<RepoTree repos={REPOS} selectedRepoId="tienda" cards={cards()} onOpen={() => {}} />);
    expect(row("Worktrees", 2)).toBeInTheDocument();
    rerender(<RepoTree repos={REPOS} selectedRepoId={null} cards={[]} onOpen={() => {}} />);
    expect(screen.queryByRole("treeitem", { name: "Worktrees" })).toBeNull();
    for (const r of screen.getAllByRole("treeitem")) expect(r).toHaveAttribute("aria-selected", "false");
  });

  it("switching the open repo expands the new one", () => {
    render(<Harness initial="tienda" />);
    fireEvent.click(row("notas"));
    expect(row("notas")).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("treeitem", { name: "Worktrees" })).toBeNull();
  });

  it("is busy while the repo list is not in", () => {
    render(<RepoTree repos={undefined} selectedRepoId={null} cards={[]} onOpen={() => {}} />);
    expect(screen.getByRole("tree")).toHaveAttribute("aria-busy", "true");
  });

  it("speaks Spanish", async () => {
    await setLang("es");
    render(<Harness initial="tienda" repos={[repo("tienda", { attention: { conflicts: { state: "counted", count: 2 }, denials: { state: "unavailable" } } })]} />);
    expect(screen.getByRole("img", { name: "2 conflictos previstos" })).toBeInTheDocument();
    expect(row("Copias de trabajo", 2)).toBeInTheDocument();
  });

  it.each(["en", "es"] as const)("axe finds nothing at 1440 and 1200 px in %s", async (lang) => {
    await setLang(lang);
    const repos = [repo("a", { state: "unavailable", tier: null }), repo("notas", { attention: { conflicts: { state: "counted", count: 1 }, denials: { state: "counted", count: 0 } } }), repo("tienda")];
    for (const width of [1440, 1200]) {
      setViewportWidth(width);
      const { container, unmount } = render(<Harness repos={repos} initial="tienda" />);
      const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
      expect(results.violations.map((v) => `${width} ${lang}: ${v.id}: ${v.help}`)).toEqual([]);
      unmount();
    }
  });
});
