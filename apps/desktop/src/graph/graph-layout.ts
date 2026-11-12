import type { GraphGeometry, GraphLayer, GraphModel, GraphRow } from "./graph-types";

export type { GraphGeometry, GraphLayer };

/** D20: the gap between lane axes and the node radius, with the floors the column shrinks to. */
export const LANE_GAP = 16;
export const LANE_GAP_MIN = 8;
export const NODE_R = 4;
export const NODE_R_MIN = 3;
/** Below this gap the nodes use the small radius. */
const SMALL_NODE_GAP = 10;
/** Value of `--gr-size-graph-row` (primitives-spec § 2.15): the canvas and the virtual window need it in JS. */
export const ROW_PX = 44;
/** Value of `--gr-size-graph-header`: the sticky header above the first row. */
export const HEADER_PX = 38;

const laneKeyOf = (row: GraphRow): string => (row.kind === "base" ? "base" : row.laneKey);

/** Vertical centre of row `index` inside a canvas that starts at the first row of `window`. */
export const rowY = (geometry: GraphGeometry, window: { first: number }, index: number): number => (index - window.first + 0.5) * geometry.rowHeight;

/**
 * Where every column and row of the canvas goes (D5, D20). The base sits at half a gap; lane `i` one gap
 * further, with the gap shrinking to fit `graphWidthPx` down to `LANE_GAP_MIN`; the lanes that still do not fit
 * get no column and are counted in `clipped`. A lane spans from its first row (WIP or head) to its fork row.
 */
export function graphLayout(model: GraphModel, graphWidthPx: number): GraphGeometry {
  const gap = Math.max(LANE_GAP_MIN, Math.min(LANE_GAP, (graphWidthPx - LANE_GAP) / (model.lanes.length + 1)));
  const baseX = LANE_GAP / 2;
  const laneX = new Map<string, number>([["base", baseX]]);
  let clipped = 0;
  for (const lane of model.lanes) {
    const x = baseX + (lane.column + 1) * gap;
    if (x > graphWidthPx) clipped += 1;
    else laneX.set(lane.key, x);
  }

  const rowIndex = new Map<string, number>();
  const spans = new Map<string, { first: number; last: number }>();
  let lastFork = -1;
  model.rows.forEach((row, i) => {
    rowIndex.set(row.id, i);
    const key = laneKeyOf(row);
    const span = spans.get(key);
    if (span === undefined) spans.set(key, { first: i, last: i });
    else if (row.kind !== "base") span.last = i;
    if (row.kind === "fork") {
      lastFork = i;
      const own = spans.get(key);
      if (own) own.last = i;
    }
  });
  const base = spans.get("base");
  if (base !== undefined) base.last = Math.max(base.last, lastFork, model.rows.findIndex((r) => r.kind === "base"));

  return {
    rowHeight: ROW_PX,
    graphWidth: graphWidthPx,
    laneX,
    laneSpan: spans,
    rowIndex,
    nodeRadius: gap < SMALL_NODE_GAP ? NODE_R_MIN : NODE_R,
    clipped,
  };
}
