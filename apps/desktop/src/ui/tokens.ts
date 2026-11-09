/**
 * Reads a pixel token (`--gr-space-1`, `--gr-space-2`, `--gr-window-margin`) for the positioning that has
 * to run in JS. The only way a number in code comes from a token.
 */
export function readPx(name: "--gr-space-1" | "--gr-space-2" | "--gr-window-margin", el?: Element): number {
  return parseFloat(getComputedStyle(el ?? document.documentElement).getPropertyValue(name)) || 0;
}
