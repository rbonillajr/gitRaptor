import { useEffect, useRef, useState } from "react";
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

/**
 * The lane drawing of the Graph column (D12): one `<canvas>` over the painted rows, outside React's tree of
 * nodes. It repaints in one animation frame when the model, the window, the width, the layers or the theme change,
 * and runs no loop otherwise (NFR-13.8). It is hidden from assistive tech: the rows say everything in text.
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

  useEffect(() => {
    latest.current = { model, geometry, rows: { first: rows.first, last: rows.last }, layers, colors };
    painter.current?.request();
  }, [model, geometry, rows.first, rows.last, layers, colors]);

  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      style={{ top: HEADER_PX + rows.first * geometry.rowHeight, left: leftPx, width: geometry.graphWidth, height: heightPx }}
      className="pointer-events-none absolute"
    />
  );
}
