/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-TREE-TABLE (TS-DSK-003 T006): WAI-ARIA tree keyboard (primitives-spec § 2.9), flat rows (D10), axe.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Tree, type TreeNode } from "./Tree";

const NODES: TreeNode[] = [
  {
    id: "r1", kind: "repo", label: "gitraptor",
    children: [
      { id: "g1", kind: "group", label: "Worktrees", children: [
        { id: "w1", kind: "item", label: "claude-1", accessibleName: "claude-1, active, branch feat-pagos" },
        { id: "w2", kind: "item", label: "feat-pagos", disabled: true },
      ] },
      { id: "g2", kind: "group", label: "Branches", children: [{ id: "b1", kind: "item", label: "main" }] },
    ],
  },
  { id: "r2", kind: "repo", label: "another", children: [{ id: "g3", kind: "group", label: "Remotes", children: [{ id: "x1", kind: "item", label: "origin" }] }] },
];

interface Spies { onSelect: (id: string) => void; onActivate: (id: string) => void; onExpandedChange: (s: ReadonlySet<string>) => void }
function Harness({ initial = ["r1", "g1"], selected = null, loading, spies }: { initial?: string[]; selected?: string | null; loading?: boolean; spies: Spies }) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set(initial));
  return (
    <Tree
      label="Repositories"
      nodes={NODES}
      expanded={expanded}
      onExpandedChange={(next: ReadonlySet<string>) => { setExpanded(next); spies.onExpandedChange(next); }}
      selectedId={selected}
      onSelect={spies.onSelect}
      onActivate={spies.onActivate}
      {...(loading === undefined ? {} : { loading })}
    />
  );
}
const spies = (): Spies => ({ onSelect: vi.fn(), onActivate: vi.fn(), onExpandedChange: vi.fn() });
const row = (name: string | RegExp) => screen.getByRole("treeitem", { name });
const focusRow = (name: string | RegExp) => act(() => row(name).focus());
const press = (key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(document.activeElement!, { key, ...init });
const activeLabel = () => document.activeElement?.textContent ?? "";

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-TREE-TABLE Tree structure and ARIA", () => {
  it("TS003-TREE-TABLE: a labelled tree whose visible rows are flat direct children with level, setsize and posinset", () => {
    render(<Harness spies={spies()} />);
    const tree = screen.getByRole("tree", { name: "Repositories" });
    const items = screen.getAllByRole("treeitem");
    expect(items.map((i) => i.textContent?.replace(/\s+/g, "")), "visible rows in depth-first order").toEqual(
      expect.arrayContaining(["gitraptor", "Worktrees", "Branches", "another"]),
    );
    expect(items, "r1, g1, w1, w2, g2, r2 are visible").toHaveLength(6);
    for (const item of items) expect(item.parentElement, "D10: no nested role=group").toBe(tree);
    const attrs = items.map((i) => [i.getAttribute("aria-level"), i.getAttribute("aria-posinset"), i.getAttribute("aria-setsize")]);
    expect(attrs).toEqual([
      ["1", "1", "2"], ["2", "1", "2"], ["3", "1", "2"], ["3", "2", "2"], ["2", "2", "2"], ["1", "2", "2"],
    ]);
  });

  it("TS003-TREE-TABLE: aria-expanded only on parents, aria-selected on the selected row, accessibleName as the name", () => {
    render(<Harness spies={spies()} selected="w1" />);
    expect(row("gitraptor")).toHaveAttribute("aria-expanded", "true");
    expect(row("Branches")).toHaveAttribute("aria-expanded", "false");
    expect(row(/claude-1, active/).hasAttribute("aria-expanded"), "leaves have no aria-expanded").toBe(false);
    expect(row(/claude-1, active/)).toHaveAttribute("aria-selected", "true");
    expect(row("Branches")).toHaveAttribute("aria-selected", "false");
    expect(row("feat-pagos"), "disabled node").toHaveAttribute("aria-disabled", "true");
  });

  it("TS003-TREE-TABLE: one Tab stop: the selected row, else the first", () => {
    const { unmount } = render(<Harness spies={spies()} />);
    expect(screen.getAllByRole("treeitem").filter((i) => i.tabIndex === 0).map((i) => i.textContent)).toHaveLength(1);
    expect(row("gitraptor").tabIndex, "first row is the stop without selection").toBe(0);
    unmount();
    render(<Harness spies={spies()} selected="g1" />);
    expect(row("Worktrees").tabIndex, "selected row is the stop").toBe(0);
    expect(row("gitraptor").tabIndex).toBe(-1);
  });

  it("TS003-TREE-TABLE: loading shows aria-busy and the sr-only text instead of rows", async () => {
    render(<Harness spies={spies()} loading />);
    expect(screen.getByRole("tree")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Loading repositories…")).toBeInTheDocument();
    expect(screen.queryAllByRole("treeitem"), "no rows while loading").toHaveLength(0);
    await act(async () => void (await i18n.changeLanguage("es")));
    expect(screen.getByText("Cargando repositorios…")).toBeInTheDocument();
  });
});

describe("TS003-TREE-TABLE Tree keyboard", () => {
  it("TS003-TREE-TABLE: ArrowDown and ArrowUp move between visible rows; Home and End jump", () => {
    render(<Harness spies={spies()} />);
    focusRow("gitraptor");
    press("ArrowDown");
    expect(activeLabel()).toContain("Worktrees");
    press("ArrowDown");
    expect(activeLabel()).toContain("claude-1");
    press("ArrowUp");
    expect(activeLabel()).toContain("Worktrees");
    press("End");
    expect(activeLabel(), "End goes to the last visible row").toContain("another");
    press("Home");
    expect(activeLabel(), "Home goes to the first row").toContain("gitraptor");
  });

  it("TS003-TREE-TABLE: ArrowRight opens a closed parent, then enters the first child; does nothing on a leaf", () => {
    const s = spies();
    render(<Harness spies={s} />);
    focusRow("Branches");
    press("ArrowRight");
    expect(row("Branches"), "ArrowRight opens a closed parent").toHaveAttribute("aria-expanded", "true");
    expect(document.activeElement, "focus stays on the parent after opening").toBe(row("Branches"));
    press("ArrowRight");
    expect(activeLabel(), "ArrowRight on an open parent enters its first child").toContain("main");
    press("ArrowRight");
    expect(activeLabel(), "ArrowRight on a leaf does nothing").toContain("main");
  });

  it("TS003-TREE-TABLE: ArrowLeft closes an open parent; on a closed parent or a leaf it goes to the parent", () => {
    render(<Harness spies={spies()} />);
    focusRow(/claude-1/);
    press("ArrowLeft");
    expect(activeLabel(), "ArrowLeft on a leaf goes to its parent").toContain("Worktrees");
    press("ArrowLeft");
    expect(row("Worktrees"), "ArrowLeft on an open parent closes it").toHaveAttribute("aria-expanded", "false");
    press("ArrowLeft");
    expect(activeLabel(), "ArrowLeft on a closed parent goes to its parent").toContain("gitraptor");
  });

  it("TS003-TREE-TABLE: Enter selects, activates and toggles a repo; Space only selects", () => {
    const s = spies();
    render(<Harness spies={s} />);
    focusRow("gitraptor");
    press(" ");
    expect(s.onSelect, "Space selects").toHaveBeenCalledWith("r1");
    expect(s.onActivate, "Space does not activate").not.toHaveBeenCalled();
    expect(row("gitraptor"), "Space does not toggle").toHaveAttribute("aria-expanded", "true");
    press("Enter");
    expect(s.onSelect).toHaveBeenLastCalledWith("r1");
    expect(s.onActivate, "Enter activates").toHaveBeenCalledWith("r1");
    expect(row("gitraptor"), "Enter toggles a repo").toHaveAttribute("aria-expanded", "false");
  });

  it("TS003-TREE-TABLE: Enter on an item activates without toggling", () => {
    const s = spies();
    render(<Harness spies={s} />);
    focusRow(/claude-1/);
    press("Enter");
    expect(s.onActivate).toHaveBeenCalledWith("w1");
    expect(s.onExpandedChange, "an item has nothing to toggle").not.toHaveBeenCalled();
  });

  it("TS003-TREE-TABLE: * opens every sibling of the focused row", () => {
    render(<Harness spies={spies()} initial={["r1"]} />);
    focusRow("Worktrees");
    press("*");
    expect(row("Worktrees")).toHaveAttribute("aria-expanded", "true");
    expect(row("Branches"), "the sibling group opens too").toHaveAttribute("aria-expanded", "true");
  });

  it("TS003-TREE-TABLE: a printable character jumps to the next label that starts with it", () => {
    render(<Harness spies={spies()} />);
    focusRow("gitraptor");
    press("b");
    expect(activeLabel(), "'b' jumps to Branches").toContain("Branches");
  });

  it("TS003-TREE-TABLE: type-ahead starts after the focused row and skips rows above it", () => {
    render(<Harness spies={spies()} />);
    focusRow("gitraptor");
    press("a");
    expect(activeLabel(), "'a' jumps to the next label that starts with a").toContain("another");
  });

  it("TS003-TREE-TABLE: selection does not follow the focus", () => {
    const s = spies();
    render(<Harness spies={s} />);
    focusRow("gitraptor");
    press("ArrowDown");
    press("ArrowDown");
    expect(s.onSelect, "moving the focus must not select").not.toHaveBeenCalled();
  });
});

describe("TS003-TREE-TABLE Tree pointer", () => {
  it("TS003-TREE-TABLE: clicking a group row selects and toggles it", () => {
    const s = spies();
    render(<Harness spies={s} />);
    fireEvent.click(row("Branches"));
    expect(s.onSelect).toHaveBeenCalledWith("g2");
    expect(row("Branches")).toHaveAttribute("aria-expanded", "true");
  });

  it("TS003-TREE-TABLE: clicking the chevron toggles without selecting", () => {
    const s = spies();
    render(<Harness spies={s} />);
    const chevron = row("Branches").querySelector("svg");
    expect(chevron, "a parent row shows a chevron glyph").not.toBeNull();
    fireEvent.click(chevron!);
    expect(row("Branches"), "chevron click toggles").toHaveAttribute("aria-expanded", "true");
    expect(s.onSelect, "chevron click must not select").not.toHaveBeenCalled();
  });

  it("TS003-TREE-TABLE: clicking an item selects it", () => {
    const s = spies();
    render(<Harness spies={s} />);
    fireEvent.click(row(/claude-1/));
    expect(s.onSelect).toHaveBeenCalledWith("w1");
  });
});

describe("TS003-TREE-TABLE Tree accessibility", () => {
  it.each(["en", "es"] as const)("TS003-TREE-TABLE: no axe violations in %s (open, loading)", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    const { container, rerender } = render(<Harness spies={spies()} selected="w1" />);
    const open = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(open.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
    rerender(<Harness spies={spies()} loading />);
    const loading = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(loading.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
