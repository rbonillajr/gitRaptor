import type { GraphColors } from "../graph-types";

export type { GraphColors };

const LANES = 7;

/**
 * The colours of the canvas, read from the design tokens (`--gr-lane-base`, `--gr-lane-1` … `7`, `--gr-text-muted`,
 * `--gr-danger`): no colour literal lives in the code (ADR-GRP-003). Read again when the theme changes.
 */
export function readGraphColors(root: Element = document.documentElement): GraphColors {
  const style = getComputedStyle(root);
  const token = (name: string) => style.getPropertyValue(name).trim();
  return {
    base: token("--gr-lane-base"),
    lane: Array.from({ length: LANES }, (_, i) => token(`--gr-lane-${i + 1}`)),
    muted: token("--gr-text-muted"),
    danger: token("--gr-danger"),
  };
}
