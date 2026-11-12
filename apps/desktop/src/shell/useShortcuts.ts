import { useEffect, useEffectEvent } from "react";

/** A hook call registers only the shortcuts it passes: the shell the window ones, `GraphView` its own. */
export interface ShortcutHandlers {
  /** mod + B. */
  onToggleSidebar?: () => void;
  /** mod + Alt + B. */
  onToggleDetail?: () => void;
  /** Escape. */
  onEscape?: () => void;
  /** mod + Alt + T (D21): graph and table view. Ignored while a text field has the focus. */
  onToggleGraphView?: () => void;
}

/** mod is ⌘ on macOS and Ctrl elsewhere. */
const isMod = (event: KeyboardEvent) => (navigator.userAgent.includes("Mac") ? event.metaKey : event.ctrlKey);

const isTextField = (target: EventTarget | null) =>
  target instanceof HTMLElement && (target.isContentEditable || target instanceof HTMLTextAreaElement || (target instanceof HTMLInputElement && !["checkbox", "radio", "button"].includes(target.type)));

/**
 * Window shortcuts of the shell. Compares `event.code`, because ⌥ changes `event.key` on macOS
 * (⌥⌘B gives `∫`).
 */
export function useShortcuts(handlers: ShortcutHandlers): void {
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.code === "KeyB" && isMod(event) && (event.altKey ? handlers.onToggleDetail : handlers.onToggleSidebar)) {
      event.preventDefault();
      if (event.altKey) handlers.onToggleDetail?.();
      else handlers.onToggleSidebar?.();
    } else if (event.code === "KeyT" && isMod(event) && event.altKey && handlers.onToggleGraphView && !isTextField(event.target)) {
      event.preventDefault();
      handlers.onToggleGraphView();
    } else if (event.key === "Escape" && !event.defaultPrevented) {
      // A layer that closed on this Esc marks it consumed (D18); one Esc closes one layer.
      handlers.onEscape?.();
    }
  });

  useEffect(() => {
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
