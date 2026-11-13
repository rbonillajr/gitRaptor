import { useEffect, useMemo, useRef, useState } from "react";
import { HEADER_PX } from "./graph-layout";
import { readGraphColors } from "./canvas/colors";
import { createPainter, paint } from "./canvas/paint";
import type { GraphColors, GraphGeometry, GraphLayer, GraphModel } from "./graph-types";

export interface GraphCanvasProps {
  model: GraphModel;
  geometry: GraphGeometry;
  /** The rows the DOM paints; the canvas covers exactly them. */
  window: { first: number; last: number };
  layers: readonly GraphLayer[];
  /** Left edge of the Graph column inside the grid. */
  leftPx: number;
}

/** The token colours, read again when `data-theme` changes or the system theme flips under `auto`. */
function useGraphColors(): GraphColors {
  const [colors, setColors] = useState(() => readGraphColors());
  useEffect(() => {
    const read = () => setColors(readGraphColors());
    read();
    const observer = new MutationObserver(read);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", read);
    return () => {
      observer.disconnect();
      media.removeEventListener("change", read);
    };
  }, []);
  return colors;
}

/** What `paint` reads of the model: the row ids and merges and the lane columns and colours, never the text. */
function shapeKeyOf(model: GraphModel, geometry: GraphGeometry): string {
  const rows = model.rows.map((r) => (r.kind === "commit" && r.merge ? `${r.id}+` : r.id)).join("\u0000");
  const lanes = model.lanes.map((l) => `${l.key}:${l.column}:${l.colorSlot ?? "-"}:${l.muted ? 1 : 0}`).join("\u0000");
  return `${model.state}|${geometry.graphWidth}|${geometry.rowHeight}|${rows}|${lanes}`;
}

/**
 * The lane drawing of the Graph column (D12): one `<canvas>` over the painted rows, outside React's tree of
 * nodes. It repaints in one animation frame when the shape of the model (not its text or its identity), the window,
 * the width, the layers or the theme change, and runs no loop otherwise (NFR-13.8): the one-second clock of the
 * fleet rebuilds the model and the layers with the same content and must not repaint. `layers` must keep its
 * identity while its content does not. It is hidden from assistive tech: the rows say everything in text.
 */
export function GraphCanvas({ model, geometry, window: rows, layers, leftPx }: GraphCanvasProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const colors = useGraphColors();
  const latest = useRef({ model, geometry, rows, layers, colors });
  const painter = useRef<ReturnType<typeof createPainter> | null>(null);

  const rowCount = rows.last - rows.first + 1;
  const heightPx = rowCount * geometry.rowHeight;

  useEffect(() => {
    painter.current = createPainter(() => {
      const canvas = ref.current;
      const ctx = canvas?.getContext("2d");
      if (!canvas || !ctx) return;
      const { model: m, geometry: g, rows: w, layers: l, colors: c } = latest.current;
      const ratio = window.devicePixelRatio || 1;
      const width = Math.round(g.graphWidth * ratio);
      const height = Math.round((w.last - w.first + 1) * g.rowHeight * ratio);
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      paint(ctx, g, m, w, l, c);
    });
    return () => painter.current?.cancel();
  }, []);

  const shapeKey = useMemo(() => shapeKeyOf(model, geometry), [model, geometry]);
  const colorsKey = useMemo(() => JSON.stringify(colors), [colors]);

  // Keep what the next frame reads current without asking for a frame.
  useEffect(() => {
    latest.current = { model, geometry, rows: { first: rows.first, last: rows.last }, layers, colors };
  });

  useEffect(() => {
    painter.current?.request();
  }, [shapeKey, colorsKey, rows.first, rows.last, layers]);

  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      style={{ top: HEADER_PX + rows.first * geometry.rowHeight, left: leftPx, width: geometry.graphWidth, height: heightPx }}
      className="pointer-events-none absolute"
    />
  );
}
