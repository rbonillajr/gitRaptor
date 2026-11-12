import type { GraphGeometry, GraphLayer, GraphModel } from "./graph-types";

export type { GraphGeometry, GraphLayer };

/** STUB (T001): the real geometry is T003. */
export function graphLayout(model: GraphModel, graphWidthPx: number): GraphGeometry {
  void model;
  void graphWidthPx;
  throw new Error("not implemented");
}
