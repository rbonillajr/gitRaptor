import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export interface FocusTrapOptions {
  initialFocus?: () => HTMLElement | null;
  returnFocus?: RefObject<HTMLElement | null>;
}

/**
 * Keeps Tab and Shift+Tab inside `ref` while `active`. On activation it focuses `initialFocus()`, else the
 * first `[data-autofocus]`, else the first focusable; on deactivation it gives the focus back to `returnFocus`
 * or to whatever had it when the trap turned on.
 */
export function useFocusTrap(ref: RefObject<HTMLElement | null>, active: boolean, options?: FocusTrapOptions): void {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    const container = ref.current;
    if (!active || !container) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusables = () => Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE));

    const target =
      optionsRef.current?.initialFocus?.() ?? container.querySelector<HTMLElement>("[data-autofocus]") ?? focusables()[0] ?? container;
    target.focus();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const items = focusables();
      const first = items[0];
      const last = items[items.length - 1];
      if (!first || !last) {
        e.preventDefault();
        return;
      }
      const current = document.activeElement;
      if (!(current instanceof Node) || !container.contains(current)) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && current === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && current === last) {
        e.preventDefault();
        first.focus();
      }
    };
    container.addEventListener("keydown", onKeyDown);
    return () => {
      container.removeEventListener("keydown", onKeyDown);
      (optionsRef.current?.returnFocus?.current ?? previous)?.focus();
    };
  }, [ref, active]);
}
