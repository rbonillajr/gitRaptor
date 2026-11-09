import { useEffect } from "react";
import type { Theme } from "@/state/ui-store";

/** Syncs the theme preference to `<html data-theme>`: `auto` removes it so `prefers-color-scheme` rules. */
export function useTheme(theme: Theme): void {
  useEffect(() => {
    const root = document.documentElement;
    if (theme === "auto") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
  }, [theme]);
}
