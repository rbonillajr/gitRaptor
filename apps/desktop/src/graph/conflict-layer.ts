import type { ConflictBandModel, SideRef } from "@/conflicts";
import { rowY } from "./graph-layout";
import type { GraphGeometry, GraphLayer } from "./graph-types";

/** DSF-DSK-001 § 4.b: the same 1.5 px as the lane lines. */
const LINE_WIDTH = 1.5;
const DASH: number[] = [4, 3];

const laneKey = (side: SideRef): string => (side.kind === "base" ? "base" : side.key);

/**
 * Row of the newest head of a lane, or of its summary row: the first row of the lane that is a commit or a lane
 * row (rows are newest first). The base is its own row. `null` when the lane has no row (it is not in the grid).
 */
function headRow(geometry: GraphGeometry, key: string): number | null {
  if (key === "base") return geometry.rowIndex.get("base") ?? null;
  let found: number | null = null;
  for (const [id, index] of geometry.rowIndex) {
    if ((id.startsWith(`commit:${key}:`) || id === `lane:${key}`) && (found === null || index < found)) found = index;
  }
  return found;
}

/**
 * The ⚡ line of every predicted conflict (D13, T011): a horizontal segment between the columns of the two lanes
 * (or up to the base line) at the row of the newer head, in `danger`; dashed when the pair is stale or another
 * lane sits between the two columns. An overlap (⚠) is a badge only, and a lane with no column (cut or clipped)
 * draws no line: the badge and the panel still say it. Pure: it keeps nothing between draws.
 */
export function conflictLayer(bands: readonly ConflictBandModel[]): GraphLayer {
  return {
    id: "conflicts",
    draw(ctx, geometry, window, colors) {
      for (const band of bands) {
        if (band.level !== "conflict") continue;
        const [ka, kb] = [laneKey(band.a), laneKey(band.b)];
        const xa = geometry.laneX.get(ka);
        const xb = geometry.laneX.get(kb);
        const ra = headRow(geometry, ka);
        const rb = headRow(geometry, kb);
        if (xa === undefined || xb === undefined || ra === null || rb === null) continue;
        const row = Math.min(ra, rb);
        if (row < window.first || row > window.last) continue;
        const [lo, hi] = xa < xb ? [xa, xb] : [xb, xa];
        const crowded = [...geometry.laneX.values()].some((x) => x > lo && x < hi);
        const y = rowY(geometry, window, row);
        ctx.strokeStyle = colors.danger;
        ctx.lineWidth = LINE_WIDTH;
        ctx.setLineDash(band.stale || crowded ? DASH : []);
        ctx.beginPath();
        ctx.moveTo(xa, y);
        ctx.lineTo(xb, y);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    },
  };
}
