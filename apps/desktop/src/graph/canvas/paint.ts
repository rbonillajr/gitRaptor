import { rowY } from "../graph-layout";
import type { GraphColors, GraphGeometry, GraphLayer, GraphModel, GraphRow } from "../graph-types";

export type Window = { first: number; last: number };

/** DSF-DSK-001 § 4.b: the lane line is 1.5 px. */
const LINE_WIDTH = 1.5;
const HALF = 2;

/** Colour of a lane: its agent's slot (1 to 7), muted without agent, the base colour for the trunk. */
function laneColor(model: GraphModel, colors: GraphColors, key: string): string {
  if (key === "base") return colors.base;
  const lane = model.lanes.find((l) => l.key === key);
  if (lane === undefined || lane.muted || lane.colorSlot === null) return colors.muted;
  return colors.lane[Math.min(Math.max(lane.colorSlot, 1), colors.lane.length) - 1] ?? colors.muted;
}

const laneKeyOf = (row: GraphRow): string => (row.kind === "base" ? "base" : row.laneKey);

function circle(ctx: CanvasRenderingContext2D, x: number, y: number, r: number) {
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * HALF);
}

/**
 * Paints the rows of `window` (the canvas starts at its first row): the base line, the lane lines with their
 * curve to the base at the fork row, then each layer in order (T011 registers the conflict line), then the
 * nodes: filled for a commit, hollow for WIP, double for a merge. It paints shapes only, never text.
 */
export function paint(
  ctx: CanvasRenderingContext2D,
  geometry: GraphGeometry,
  model: GraphModel,
  window: Window,
  layers: readonly GraphLayer[],
  colors: GraphColors,
): void {
  const { rowHeight, laneX, laneSpan, nodeRadius } = geometry;
  ctx.clearRect(0, 0, geometry.graphWidth, (window.last - window.first + 1) * rowHeight);
  const y = (index: number) => rowY(geometry, window, index);
  const top = y(window.first) - rowHeight / HALF;
  const bottom = y(window.last) + rowHeight / HALF;
  const baseX = laneX.get("base");

  ctx.lineWidth = LINE_WIDTH;
  ctx.lineCap = "round";
  const line = (key: string, x: number, from: number, to: number) => {
    const first = Math.max(from, window.first);
    const last = Math.min(to, window.last);
    if (first > last) return;
    ctx.strokeStyle = laneColor(model, colors, key);
    ctx.beginPath();
    ctx.moveTo(x, from < window.first ? top : y(first));
    ctx.lineTo(x, to > window.last ? bottom : y(last));
    ctx.stroke();
  };

  if (baseX !== undefined) {
    const span = laneSpan.get("base");
    if (span) line("base", baseX, span.first, span.last);
  }
  for (const lane of model.lanes) {
    const x = laneX.get(lane.key);
    const span = laneSpan.get(lane.key);
    if (x === undefined || span === undefined) continue;
    line(lane.key, x, span.first, span.last);
    if (baseX !== undefined && span.last >= window.first && span.last <= window.last) {
      // The way out of the lane: a curve from its axis back to the base line at the fork row.
      const fy = y(span.last);
      ctx.strokeStyle = laneColor(model, colors, lane.key);
      ctx.beginPath();
      ctx.moveTo(x, fy - rowHeight / HALF);
      ctx.quadraticCurveTo(x, fy, baseX, fy);
      ctx.stroke();
    }
  }

  for (const layer of layers) layer.draw(ctx, geometry, window, colors);

  const last = Math.min(window.last, model.rows.length - 1);
  for (let i = Math.max(window.first, 0); i <= last; i += 1) {
    const row = model.rows[i];
    if (row === undefined || row.kind === "fork" || row.kind === "more") continue;
    const key = laneKeyOf(row);
    const x = laneX.get(key);
    if (x === undefined) continue;
    const color = laneColor(model, colors, key);
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    if (row.kind === "wip") {
      circle(ctx, x, y(i), nodeRadius);
      ctx.stroke();
    } else if (row.kind === "commit" && row.merge) {
      circle(ctx, x, y(i), nodeRadius);
      ctx.stroke();
      circle(ctx, x, y(i), nodeRadius / HALF);
      ctx.fill();
    } else {
      circle(ctx, x, y(i), nodeRadius);
      ctx.fill();
    }
  }
}

/**
 * Coalesces repaint requests into one animation frame (NFR-13.8): nothing is scheduled until something asks,
 * so an idle canvas runs no loop.
 */
export function createPainter(draw: () => void): { request: () => void; cancel: () => void } {
  let frame: number | null = null;
  return {
    request() {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        draw();
      });
    },
    cancel() {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
    },
  };
}
