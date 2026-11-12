import "@testing-library/jest-dom/vitest";
import { beforeEach } from "vitest";

type Listener = (event: MediaQueryListEvent) => void;

let viewportWidth = 1440;
const lists = new Set<{ query: string; listeners: Set<Listener>; matches: boolean }>();

/** Evaluates the `min-width` / `max-width` parts of a media query against the simulated width. */
function evaluate(query: string, width: number): boolean {
  const min = /\(\s*min-width:\s*(\d+(?:\.\d+)?)px\s*\)/.exec(query);
  const max = /\(\s*max-width:\s*(\d+(?:\.\d+)?)px\s*\)/.exec(query);
  if (!min && !max) return false;
  return (!min || width >= Number(min[1])) && (!max || width <= Number(max[1]));
}

/**
 * Sets the width the simulated `matchMedia` answers for (default 1440) and notifies the lists whose
 * result changes.
 */
export function setViewportWidth(px: number): void {
  viewportWidth = px;
  for (const list of lists) {
    const matches = evaluate(list.query, px);
    if (matches === list.matches) continue;
    list.matches = matches;
    const event = { matches, media: list.query } as MediaQueryListEvent;
    for (const listener of list.listeners) listener(event);
  }
}

function matchMedia(query: string): MediaQueryList {
  const entry = { query, listeners: new Set<Listener>(), matches: evaluate(query, viewportWidth) };
  lists.add(entry);
  const add = (_type: string, listener: Listener) => entry.listeners.add(listener);
  const remove = (_type: string, listener: Listener) => entry.listeners.delete(listener);
  return {
    get matches() {
      return entry.matches;
    },
    media: query,
    onchange: null,
    addEventListener: add,
    removeEventListener: remove,
    addListener: (listener: Listener) => entry.listeners.add(listener),
    removeListener: (listener: Listener) => entry.listeners.delete(listener),
    dispatchEvent: () => false,
  } as unknown as MediaQueryList;
}

Object.defineProperty(window, "matchMedia", { configurable: true, writable: true, value: matchMedia });

/** jsdom has no layout engine and no `ResizeObserver`: a no-op that never reports, so a size is only ever read at mount. */
class NoopResizeObserver implements ResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
if (typeof globalThis.ResizeObserver === "undefined") globalThis.ResizeObserver = NoopResizeObserver;

beforeEach(() => {
  localStorage.clear();
});
