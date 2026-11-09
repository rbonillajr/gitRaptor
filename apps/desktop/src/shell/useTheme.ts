import { useLayoutEffect } from "react";
import type { Theme } from "@/state/ui-store";

/** Syncs the theme preference to `<html data-theme>`: `auto` removes it so `prefers-color-scheme` rules. */
export function useTheme(theme: Theme): void {
  // Before paint, so a stored theme never flashes the system one on the first frame.
  useLayoutEffect(() => {
    const root = document.documentElement;
    if (theme === "auto") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
  }, [theme]);
}
