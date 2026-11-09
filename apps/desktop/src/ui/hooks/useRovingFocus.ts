import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";

export interface RovingOptions {
  orientation: "vertical" | "horizontal" | "both";
  loop: boolean;
  /** Skips the items with `aria-disabled="true"`. */
  skipDisabled: boolean;
  /** Called after the focus moved, with the new index. */
  onMove?: (index: number) => void;
}

const PREV: Record<RovingOptions["orientation"], string[]> = {
  vertical: ["ArrowUp"],
  horizontal: ["ArrowLeft"],
  both: ["ArrowUp", "ArrowLeft"],
};
const NEXT: Record<RovingOptions["orientation"], string[]> = {
  vertical: ["ArrowDown"],
  horizontal: ["ArrowRight"],
  both: ["ArrowDown", "ArrowRight"],
};

/**
 * One Tab stop across the items matching `itemSelector` (DOM order): the active item has `tabindex=0`, the
 * rest `-1`. Arrow keys, Home and End move the focus. The active item is the focused one, else the one with
 * `aria-checked="true"` (a radio group keeps its selection as the stop), else the last one that was active.
 * Attach `onKeyDown` to the container.
 */
export function useRovingFocus(
  containerRef: RefObject<HTMLElement | null>,
  itemSelector: string,
  options: RovingOptions,
): { onKeyDown: (e: KeyboardEvent) => void; focusIndex: (i: number) => void; activeIndex: number } {
  const [activeIndex, setActiveIndex] = useState(0);
  const latest = useRef({ itemSelector, options });
  latest.current = { itemSelector, options };

  const items = useCallback(
    () => Array.from(containerRef.current?.querySelectorAll<HTMLElement>(latest.current.itemSelector) ?? []),
    [containerRef],
  );

  // Runs after every render: the items can change at any time (rows appear, a selection moves).
  // The state is only set when it differs, so the chain ends after one extra render.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    const list = items();
    const focusedAt = list.findIndex((el) => el === document.activeElement);
    const checkedAt = list.findIndex((el) => el.getAttribute("aria-checked") === "true");
    const next = focusedAt !== -1 ? focusedAt : checkedAt !== -1 ? checkedAt : Math.min(activeIndex, Math.max(list.length - 1, 0));
    list.forEach((el, i) => {
      el.tabIndex = i === next ? 0 : -1;
    });
    if (next !== activeIndex) setActiveIndex(next);
  });

  const focusIndex = useCallback(
    (i: number) => {
      const list = items();
      const el = list[i];
      if (!el) return;
      list.forEach((item, at) => {
        item.tabIndex = at === i ? 0 : -1;
      });
      el.focus();
      setActiveIndex(i);
    },
    [items],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const { orientation, loop, skipDisabled, onMove } = latest.current.options;
      const list = items();
      const current = list.findIndex((el) => el === document.activeElement || el.contains(document.activeElement));
      if (current === -1) return;
      const usable = (i: number) => !(skipDisabled && list[i]?.getAttribute("aria-disabled") === "true");

      let direction = 0;
      if (NEXT[orientation].includes(e.key)) direction = 1;
      else if (PREV[orientation].includes(e.key)) direction = -1;

      let target = -1;
      if (e.key === "Home" || e.key === "End") {
        const order = e.key === "Home" ? list.map((_, i) => i) : list.map((_, i) => list.length - 1 - i);
        target = order.find(usable) ?? -1;
      } else if (direction !== 0) {
        for (let step = 1; step <= list.length; step += 1) {
          const raw = current + direction * step;
          if (!loop && (raw < 0 || raw >= list.length)) break;
          const i = (raw + list.length * step) % list.length;
          if (usable(i)) {
            target = i;
            break;
          }
        }
      } else return;

      e.preventDefault();
      if (target === -1 || target === current) return;
      focusIndex(target);
      onMove?.(target);
    },
    [items, focusIndex],
  );

  return { onKeyDown, focusIndex, activeIndex };
}
