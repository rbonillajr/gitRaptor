import type { CSSProperties } from "react";

/** Joins the truthy class parts with one space. */
export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

/** `style` value that carries CSS custom properties (`--gr-…`) next to the usual properties, without a cast. */
export type CssVars = CSSProperties & Record<`--${string}`, string | number>;
