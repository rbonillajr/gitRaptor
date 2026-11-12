import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { GraphColumnId } from "./graph-types";

type OptionalColumn = Exclude<GraphColumnId, "message">;

export interface GraphPrefs {
  columns: Record<OptionalColumn, boolean>;
  view: "graph" | "table";
}

export interface GraphPrefsActions {
  setColumn: (id: OptionalColumn, on: boolean) => void;
  resetColumns: () => void;
  setView: (view: GraphPrefs["view"]) => void;
}

/** Widths of the fixed columns (primitives-spec § 2.15): constants of the graph screen, not tokens. */
export const COLUMN_PX = { branch: 195, graph: 175, author: 190, date: 120, sha: 84 } as const;
/** What Commit message needs before another column may take its room. */
export const MESSAGE_MIN_PX = 220;

const DEFAULT_COLUMNS: GraphPrefs["columns"] = { branch: true, graph: true, author: true, date: true, sha: true };
const DEFAULT_VIEW: GraphPrefs["view"] = "graph";
/** Output order of `visibleColumns`. */
const ORDER: readonly GraphColumnId[] = ["branch", "graph", "message", "author", "date", "sha"];
/** D11: the order columns fold in when the center is too narrow. `graph` and `message` never fold. */
const FOLD_ORDER: readonly OptionalColumn[] = ["sha", "author", "date", "branch"];

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * What survives of a stored value: only the known booleans of `columns` and a known `view`. Anything else (another
 * type, an unknown key, broken JSON, an inherited key) leaves the default. Never throws.
 */
function validated(stored: unknown): GraphPrefs {
  const columns = { ...DEFAULT_COLUMNS };
  let view = DEFAULT_VIEW;
  if (isRecord(stored)) {
    const saved = Object.hasOwn(stored, "columns") ? stored["columns"] : undefined;
    if (isRecord(saved)) {
      for (const id of Object.keys(DEFAULT_COLUMNS) as OptionalColumn[]) {
        const value = Object.hasOwn(saved, id) ? saved[id] : undefined;
        if (typeof value === "boolean") columns[id] = value;
      }
    }
    const savedView = Object.hasOwn(stored, "view") ? stored["view"] : undefined;
    if (savedView === "graph" || savedView === "table") view = savedView;
  }
  return { columns, view };
}

/**
 * The cosmetic preference of the graph (D10): which columns show and which view. Same for every repo; it
 * authorises nothing (ADR-GRP-017 M-3). `gitraptor.ui.v1` stays as it is.
 */
export const useGraphPrefs = create<GraphPrefs & GraphPrefsActions>()(
  persist(
    (set): GraphPrefs & GraphPrefsActions => ({
      columns: { ...DEFAULT_COLUMNS },
      view: DEFAULT_VIEW,
      setColumn: (id, on) => set((s) => ({ columns: { ...s.columns, [id]: on } })),
      resetColumns: () => set({ columns: { ...DEFAULT_COLUMNS } }),
      setView: (view) => set({ view }),
    }),
    {
      name: "gitraptor.graph.v1",
      version: 1,
      partialize: (s): GraphPrefs => ({ columns: s.columns, view: s.view }),
      merge: (stored, current) => ({ ...current, ...validated(stored) }),
    },
  ),
);

/** The columns the preference asks for, `message` always; `graph` is never hidden. */
const wanted = (prefs: GraphPrefs): GraphColumnId[] => ORDER.filter((id) => id === "message" || id === "graph" || prefs.columns[id]);

const fixedWidth = (ids: readonly GraphColumnId[]) => ids.reduce((sum, id) => sum + (id === "message" ? 0 : COLUMN_PX[id]), 0);

/** D11: the columns that show at this width of the center, in grid order. The saved preference is not touched. */
export function visibleColumns(prefs: GraphPrefs, availableWidthPx: number): GraphColumnId[] {
  let ids = wanted(prefs);
  for (const id of FOLD_ORDER) {
    if (fixedWidth(ids) + MESSAGE_MIN_PX <= availableWidthPx) break;
    ids = ids.filter((x) => x !== id);
  }
  return ids;
}

/** The columns the preference asks for and do not fit; Column settings marks them. */
export function folded(prefs: GraphPrefs, availableWidthPx: number): GraphColumnId[] {
  const shown = new Set(visibleColumns(prefs, availableWidthPx));
  return wanted(prefs).filter((id) => !shown.has(id));
}
