import { useEffect, useRef, type RefObject } from "react";

export type DismissReason = "escape" | "pointer-outside" | "focus-outside";

export interface DismissOptions {
  escape?: boolean;
  pointerOutside?: boolean;
  focusOutside?: boolean;
}

/**
 * While `active`, calls `onDismiss` on Escape, on a pointerdown outside every `inside` element and, only
 * when `focusOutside` is true, when the focus moves to an element outside all of them.
 */
export function useDismiss(
  inside: Array<RefObject<HTMLElement | null>>,
  active: boolean,
  onDismiss: (reason: DismissReason) => void,
  options?: DismissOptions,
): void {
  const latest = useRef({ inside, onDismiss, options });
  latest.current = { inside, onDismiss, options };

  useEffect(() => {
    if (!active) return;
    const isInside = (node: EventTarget | null) =>
      node instanceof Node && latest.current.inside.some((r) => r.current?.contains(node));

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && (latest.current.options?.escape ?? true)) {
        // Consumed: the shell's window listener skips an Esc that is already handled (DS-US-DSK-008 D18).
        e.preventDefault();
        latest.current.onDismiss("escape");
      }
    };
    const onPointerDown = (e: Event) => {
      if (!(latest.current.options?.pointerOutside ?? true) || isInside(e.target)) return;
      latest.current.onDismiss("pointer-outside");
    };
    const onFocusOut = (e: FocusEvent) => {
      if (!(latest.current.options?.focusOutside ?? false)) return;
      if (e.relatedTarget && isInside(e.target) && !isInside(e.relatedTarget)) latest.current.onDismiss("focus-outside");
    };

    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("focusout", onFocusOut);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("focusout", onFocusOut);
    };
  }, [active]);
}
