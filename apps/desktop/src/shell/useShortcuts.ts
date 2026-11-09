import { useEffect, useEffectEvent } from "react";

export interface ShortcutHandlers {
  /** mod + B. */
  onToggleSidebar: () => void;
  /** mod + Alt + B. */
  onToggleDetail: () => void;
  /** Escape. */
  onEscape: () => void;
}

/** mod is ⌘ on macOS and Ctrl elsewhere. */
const isMod = (event: KeyboardEvent) => (navigator.userAgent.includes("Mac") ? event.metaKey : event.ctrlKey);

/**
 * Window shortcuts of the shell. Compares `event.code`, because ⌥ changes `event.key` on macOS
 * (⌥⌘B gives `∫`).
 */
export function useShortcuts(handlers: ShortcutHandlers): void {
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.code === "KeyB" && isMod(event)) {
      event.preventDefault();
      if (event.altKey) handlers.onToggleDetail();
      else handlers.onToggleSidebar();
    } else if (event.key === "Escape" && !event.defaultPrevented) {
      // A layer that closed on this Esc marks it consumed (D18); one Esc closes one layer.
      handlers.onEscape();
    }
  });

  useEffect(() => {
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
