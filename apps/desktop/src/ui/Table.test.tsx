/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-TREE-TABLE (TS-DSK-003 T006): WAI-ARIA grid (primitives-spec § 2.15), 6 columns x 30 rows, axe.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import { Table, type TableColumn } from "./Table";

interface Row { id: string; msg: string }
const COLUMNS: TableColumn[] = [
  { id: "branch", header: "Branch / Tag", track: "195px" },
  { id: "graph", header: "Graph", track: "175px", paintHover: false },
  { id: "msg", header: "Commit message", track: "minmax(220px, 1fr)", primary: true },
  { id: "author", header: "Author", track: "190px" },
  { id: "date", header: "Date / Time", track: "120px" },
  { id: "sha", header: "Sha", track: "84px" },
];
const ROWS: Row[] = Array.from({ length: 30 }, (_, i) => ({ id: `c${i + 1}`, msg: `commit ${i + 1}` }));

interface Props { selectedId?: string | null; wipId?: string; disabled?: boolean; loading?: boolean; totalRows?: number; onSelect?: (id: string) => void; onActivate?: (id: string) => void; calls?: string[] }
function Grid(p: Props) {
  return (
    <Table<Row>
      label="Commit graph"
      columns={COLUMNS}
      rows={ROWS}
      getRowId={(r: Row) => r.id}
      getRowLabel={(r: Row) => `${r.msg} by Ana`}
      renderCell={(r: Row, col: string) =>
        col === "sha" ? <button type="button" tabIndex={-1}>copy {r.id}</button> : col === "msg" ? r.msg : col}
      selectedId={p.selectedId ?? null}
      onSelect={(id: string) => { p.calls?.push(`select:${id}`); p.onSelect?.(id); }}
      onActivate={(id: string) => { p.calls?.push(`activate:${id}`); p.onActivate?.(id); }}
      {...(p.wipId === undefined ? {} : { wipId: p.wipId })}
      {...(p.disabled === undefined ? {} : { disabled: p.disabled })}
      {...(p.loading === undefined ? {} : { loading: p.loading })}
      {...(p.totalRows === undefined ? {} : { totalRows: p.totalRows })}
      settings={<button type="button" aria-label="Column settings">gear</button>}
    />
  );
}
const row = (n: number) => screen.getByRole("row", { name: `commit ${n} by Ana` });
const focusRow = (n: number) => act(() => row(n).focus());
const press = (key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(document.activeElement!, { key, ...init });

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-TREE-TABLE Table structure", () => {
  it("TS003-TREE-TABLE: a labelled grid with aria-rowcount (rows + header) and aria-colcount", () => {
    render(<Grid />);
    const grid = screen.getByRole("grid", { name: "Commit graph" });
    expect(grid).toHaveAttribute("aria-rowcount", "31");
    expect(grid).toHaveAttribute("aria-colcount", "6");
  });

  it("TS003-TREE-TABLE: totalRows drives aria-rowcount under virtualization", () => {
    render(<Grid totalRows={500} />);
    expect(screen.getByRole("grid")).toHaveAttribute("aria-rowcount", "501");
  });

  it("TS003-TREE-TABLE: header row has aria-rowindex 1 and six columnheaders; row i has aria-rowindex i + 2", () => {
    render(<Grid />);
    expect(screen.getAllByRole("columnheader").map((h) => h.textContent)).toEqual(expect.arrayContaining(["Branch / Tag", "Sha"]));
    expect(screen.getAllByRole("columnheader")).toHaveLength(6);
    const headerRow = screen.getAllByRole("columnheader")[0]!.closest('[role="row"]')!;
    expect(headerRow).toHaveAttribute("aria-rowindex", "1");
    expect(row(1)).toHaveAttribute("aria-rowindex", "2");
    expect(row(30)).toHaveAttribute("aria-rowindex", "31");
    expect(within(row(1)).getAllByRole("gridcell"), "six gridcells per row").toHaveLength(6);
  });

  it("TS003-TREE-TABLE: the column template travels as --gr-table-columns", () => {
    render(<Grid />);
    const grid = screen.getByRole("grid");
    expect(grid.style.getPropertyValue("--gr-table-columns"), "tracks joined by a space").toBe(COLUMNS.map((c) => c.track).join(" "));
  });

  it("TS003-TREE-TABLE: the settings slot renders in the header", () => {
    render(<Grid />);
    expect(screen.getByRole("button", { name: "Column settings" })).toBeInTheDocument();
  });

  it("TS003-TREE-TABLE: one Tab stop: the selected row, else the first; aria-selected marks the selection", () => {
    const { unmount } = render(<Grid />);
    expect(screen.getAllByRole("row").filter((r) => r.tabIndex === 0)).toHaveLength(1);
    expect(row(1).tabIndex).toBe(0);
    unmount();
    render(<Grid selectedId="c4" />);
    expect(row(4).tabIndex).toBe(0);
    expect(row(4)).toHaveAttribute("aria-selected", "true");
    expect(row(1).tabIndex).toBe(-1);
  });

  it("TS003-TREE-TABLE: wipId marks that row with the accent glow", () => {
    render(<Grid wipId="c2" />);
    expect(row(2).className, "the WIP row is tinted").toContain("bg-accent-glow");
    expect(row(3).className).not.toContain("bg-accent-glow");
  });

  it("TS003-TREE-TABLE: loading shows aria-busy and the sr-only text in en and es", async () => {
    render(<Grid loading />);
    expect(screen.getByRole("grid")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Loading rows…")).toBeInTheDocument();
    await act(async () => void (await i18n.changeLanguage("es")));
    expect(screen.getByText("Cargando filas…")).toBeInTheDocument();
  });
});

describe("TS003-TREE-TABLE Table keyboard", () => {
  it("TS003-TREE-TABLE: ArrowDown, ArrowUp, Home and End move between rows", () => {
    render(<Grid />);
    focusRow(1);
    press("ArrowDown");
    expect(document.activeElement).toBe(row(2));
    press("ArrowUp");
    expect(document.activeElement).toBe(row(1));
    press("End");
    expect(document.activeElement, "End goes to the last row").toBe(row(30));
    press("Home");
    expect(document.activeElement, "Home goes to the first row").toBe(row(1));
  });

  it("TS003-TREE-TABLE: Ctrl+End and Ctrl+Home go to the last and first row", () => {
    render(<Grid />);
    focusRow(5);
    press("End", { ctrlKey: true });
    expect(document.activeElement).toBe(row(30));
    press("Home", { ctrlKey: true });
    expect(document.activeElement).toBe(row(1));
  });

  it("TS003-TREE-TABLE: PageDown and PageUp jump 10 rows", () => {
    render(<Grid />);
    focusRow(3);
    press("PageDown");
    expect(document.activeElement, "PageDown skips 10 rows").toBe(row(13));
    press("PageUp");
    expect(document.activeElement, "PageUp skips back 10 rows").toBe(row(3));
  });

  it("TS003-TREE-TABLE: Enter calls onSelect and then onActivate; click calls onSelect", () => {
    const calls: string[] = [];
    render(<Grid calls={calls} />);
    focusRow(2);
    press("Enter");
    expect(calls, "select first, activate second").toEqual(["select:c2", "activate:c2"]);
    fireEvent.click(row(5));
    expect(calls.at(-1)).toBe("select:c5");
  });

  it("TS003-TREE-TABLE: ArrowRight enters the cell button; ArrowLeft and Escape return to the row", () => {
    render(<Grid />);
    focusRow(2);
    press("ArrowRight");
    expect(document.activeElement, "ArrowRight enters the first focusable of the row").toBe(within(row(2)).getByRole("button", { name: "copy c2" }));
    press("ArrowLeft");
    expect(document.activeElement, "ArrowLeft on the first focusable returns to the row").toBe(row(2));
    press("ArrowRight");
    press("Escape");
    expect(document.activeElement, "Escape returns to the row").toBe(row(2));
  });

  it("TS003-TREE-TABLE: disabled sets aria-disabled on the grid, the keyboard still moves and Enter does not select", () => {
    const calls: string[] = [];
    render(<Grid disabled calls={calls} />);
    expect(screen.getByRole("grid")).toHaveAttribute("aria-disabled", "true");
    focusRow(1);
    press("ArrowDown");
    expect(document.activeElement, "the focus still moves when the engine is stopped").toBe(row(2));
    press("Enter");
    fireEvent.click(row(3));
    expect(calls, "a disabled grid selects nothing").toEqual([]);
  });
});

describe("TS003-TREE-TABLE Table accessibility", () => {
  it.each(["en", "es"] as const)("TS003-TREE-TABLE: no axe violations in %s (normal, wip, disabled, loading)", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    for (const p of [{ selectedId: "c2", wipId: "c1" }, { disabled: true }, { loading: true }] as Props[]) {
      const { container, unmount } = render(<Grid {...p} />);
      const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
      expect(results.violations.map((v) => `${v.id}: ${v.help}`), JSON.stringify(p)).toEqual([]);
      unmount();
    }
  });
});


// DS-US-DSK-005 T005 (D12): the additive props of a virtualised list.
describe("Table virtual window props", () => {
  interface WindowProps { first?: number; outside?: (index: number) => void; focusId?: string | null; wipIds?: ReadonlySet<string> }
  function Window({ first = 10, outside, focusId, wipIds }: WindowProps) {
    const rows = ROWS.slice(first, first + 10);
    return (
      <Table<Row>
        label="Commit graph"
        columns={COLUMNS}
        rows={rows}
        totalRows={ROWS.length}
        firstRowIndex={first}
        getRowId={(r: Row) => r.id}
        getRowLabel={(r: Row) => `${r.msg} by Ana`}
        renderCell={(r: Row, col: string) => (col === "msg" ? r.msg : col)}
        selectedId={null}
        onSelect={() => undefined}
        onActivate={() => undefined}
        {...(outside ? { onFocusOutside: outside } : {})}
        {...(focusId !== undefined ? { focusId } : {})}
        {...(wipIds ? { wipIds } : {})}
      />
    );
  }

  it("firstRowIndex offsets aria-rowindex", () => {
    render(<Window />);
    expect(row(11)).toHaveAttribute("aria-rowindex", "12");
    expect(row(20)).toHaveAttribute("aria-rowindex", "21");
    expect(screen.getByRole("grid")).toHaveAttribute("aria-rowcount", "31");
  });

  it("focus outside the window asks the caller", () => {
    const asked: number[] = [];
    render(<Window outside={(i) => asked.push(i)} />);
    focusRow(11);
    press("ArrowUp");
    expect(asked, "above the painted rows").toEqual([9]);
    focusRow(20);
    press("ArrowDown");
    press("PageDown");
    expect(asked.slice(1), "below the painted rows").toEqual([20, 29]);
    press("Home");
    press("End");
    expect(asked.slice(3), "Home is 0 and End is the last row of the list").toEqual([0, 29]);
  });

  it("inside the window the arrows move the focus without asking", () => {
    const asked: number[] = [];
    render(<Window outside={(i) => asked.push(i)} />);
    focusRow(12);
    press("ArrowDown");
    expect(document.activeElement).toBe(row(13));
    expect(asked).toEqual([]);
  });

  it("without onFocusOutside the keys stay inside the painted rows", () => {
    render(<Window />);
    focusRow(11);
    press("ArrowUp");
    expect(document.activeElement).toBe(row(11));
    press("End");
    expect(document.activeElement).toBe(row(20));
  });

  it("focusId focuses the row once it is painted", () => {
    const { rerender } = render(<Window focusId="c25" />);
    expect(document.activeElement).not.toBe(row(11));
    rerender(<Window first={20} focusId="c25" />);
    expect(document.activeElement).toBe(row(25));
  });

  it("wipIds marks every listed row as WIP", () => {
    render(<Window wipIds={new Set(["c11", "c13"])} />);
    expect(row(11)).toHaveClass("bg-accent-glow");
    expect(row(13)).toHaveClass("bg-accent-glow");
    expect(row(12)).not.toHaveClass("bg-accent-glow");
  });
});
