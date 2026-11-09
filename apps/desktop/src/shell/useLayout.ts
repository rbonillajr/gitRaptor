import { useSyncExternalStore } from "react";

/** From this width the detail is a third panel; below it, an overlay drawer (DSF-DSK-001 § 2.4). */
const WIDE_QUERY = "(min-width: 1360px)";

function subscribe(onChange: () => void): () => void {
  const list = window.matchMedia(WIDE_QUERY);
  list.addEventListener("change", onChange);
  return () => list.removeEventListener("change", onChange);
}

const isWide = () => window.matchMedia(WIDE_QUERY).matches;

export interface Layout {
  /** Three panels side by side; otherwise the detail is a drawer. */
  wide: boolean;
}

/** The window layout for the current width. */
export function useLayout(): Layout {
  return { wide: useSyncExternalStore(subscribe, isWide) };
}
