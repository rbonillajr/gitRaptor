import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { GraphColumnId } from "./graph-types";

export interface GraphPrefs {
  columns: Record<Exclude<GraphColumnId, "message">, boolean>;
  view: "graph" | "table";
}

export interface GraphPrefsActions {
  setColumn: (id: Exclude<GraphColumnId, "message">, on: boolean) => void;
  resetColumns: () => void;
  setView: (view: GraphPrefs["view"]) => void;
}

const notImplemented = (): never => {
  throw new Error("not implemented");
};

/** STUB (T001): the validating `merge` and the actions are T004. */
export const useGraphPrefs = create<GraphPrefs & GraphPrefsActions>()(
  persist(
    (): GraphPrefs & GraphPrefsActions => ({
      columns: { branch: true, graph: true, author: true, date: true, sha: true },
      view: "graph",
      setColumn: notImplemented,
      resetColumns: notImplemented,
      setView: notImplemented,
    }),
    { name: "gitraptor.graph.v1", version: 1, merge: notImplemented },
  ),
);

export function visibleColumns(prefs: GraphPrefs, availableWidthPx: number): GraphColumnId[] {
  void prefs;
  void availableWidthPx;
  return notImplemented();
}

export function folded(prefs: GraphPrefs, availableWidthPx: number): GraphColumnId[] {
  void prefs;
  void availableWidthPx;
  return notImplemented();
}
