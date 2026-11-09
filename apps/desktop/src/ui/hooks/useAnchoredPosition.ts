import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

export type Placement = "bottom-start" | "bottom-end" | "top-start" | "top-end" | "bottom" | "top";

export interface AnchoredOptions {
  placement: Placement;
  offset: number;
  padding: number;
}

/**
 * Places `floating` with `position: fixed` against the anchor's rectangle, flips to the opposite side when it
 * does not fit and shifts horizontally to stay inside the viewport padding. It writes top/left through the
 * CSSOM and `data-side` on the floating element; it recomputes on open, scroll (capture, passive) and resize.
 * Mount it where the floating element already exists: a ref still null on the first layout is skipped.
 */
export function useAnchoredPosition(
  anchor: RefObject<HTMLElement | null>,
  floating: RefObject<HTMLElement | null>,
  open: boolean,
  options: AnchoredOptions,
): void {
  const { placement, offset, padding } = options;
  const latest = useRef({ placement, offset, padding });
  latest.current = { placement, offset, padding };

  const place = () => {
    const a = anchor.current;
    const f = floating.current;
    if (!a || !f) return;
    const { placement: p, offset: gap, padding: pad } = latest.current;
    const ar = a.getBoundingClientRect();
    const fr = f.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    const wantsTop = p.startsWith("top");
    const topIfTop = ar.top - gap - fr.height;
    const topIfBottom = ar.bottom + gap;
    const fitsTop = topIfTop >= pad;
    const fitsBottom = topIfBottom + fr.height <= vh - pad;
    const side: "top" | "bottom" = wantsTop ? (fitsTop || !fitsBottom ? "top" : "bottom") : fitsBottom || !fitsTop ? "bottom" : "top";

    let left = ar.left;
    if (p.endsWith("-end")) left = ar.right - fr.width;
    else if (p === "bottom" || p === "top") left = ar.left + (ar.width - fr.width) / 2;
    left = Math.max(pad, Math.min(left, vw - pad - fr.width));

    f.style.position = "fixed";
    f.style.top = `${side === "top" ? topIfTop : topIfBottom}px`;
    f.style.left = `${left}px`;
    f.setAttribute("data-side", side);
  };

  useLayoutEffect(() => {
    if (open) place();
    // `place` only reads refs; the options it uses are mirrored in `latest`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, placement, offset, padding]);

  useEffect(() => {
    if (!open) return;
    window.addEventListener("scroll", place, { capture: true, passive: true });
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
}
