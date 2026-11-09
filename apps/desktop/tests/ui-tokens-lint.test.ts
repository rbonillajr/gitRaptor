// TS003-TOKEN-LINT (TS-DSK-003 T002): no literal colour, measure, radius or theme branch in src/ui.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const root = new URL("..", import.meta.url).pathname;
const uiDir = join(root, "src", "ui");

const ANYWHERE: Array<[string, RegExp]> = [
  ["hex colour", /#[0-9a-fA-F]{3,8}\b/],
  ["colour function", /\b(rgba?|hsla?|oklch|oklab|lab|lch|color-mix|hwb)\(/],
  ["literal measure", /\d(\.\d+)?(px|rem|em)\b/],
  ["theme branch (dark:)", /\bdark:/],
  ["radius outside the scale", /\brounded(-[trblsexy]{1,2})?-(none|xl|2xl|3xl|4xl|full|\[)/],
  ["arbitrary value in brackets", /-\[[^\]]+\]/],
];
const IN_STRINGS: Array<[string, RegExp]> = [
  [
    "numeric Tailwind scale",
    /\b-?(p|px|py|pt|pr|pb|pl|ps|pe|m|mx|my|mt|mr|mb|ml|ms|me|gap|gap-x|gap-y|size|w|h|min-w|min-h|max-w|max-h|inset|top|right|bottom|left|translate-x|translate-y)-(0\.5|[1-9]\d*(\.5)?)\b/,
  ],
  [
    "Tailwind palette colour",
    /\b(bg|text|border|outline|ring|fill|stroke|shadow|divide|caret|accent|decoration)-(slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|black|white)\b/,
  ],
  ["motion outside gr-motion*", /\b(transition(-\w+)?|duration-\d+|ease-\w+|delay-\d+|animate-\w+)\b/],
  ["opacity as state", /\bopacity-(?!0\b|100\b)\d+/],
];

function stripComments(code: string): string {
  return code.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");
}
function stringLiterals(code: string): string {
  return (code.match(/"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`/g) ?? []).join("\n");
}

/** Scans one file's text; returns `line N: pattern` findings. */
function scan(code: string): string[] {
  const found: string[] = [];
  stripComments(code)
    .split("\n")
    .forEach((line, i) => {
      for (const [name, re] of ANYWHERE) if (re.test(line)) found.push(`line ${i + 1}: ${name}`);
      for (const [name, re] of IN_STRINGS) if (re.test(stringLiterals(line))) found.push(`line ${i + 1}: ${name}`);
    });
  return found;
}

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}
const sources = () => walk(uiDir).filter((p) => /\.(ts|tsx)$/.test(p) && !/\.test\.tsx?$/.test(p));

describe("TS003-TOKEN-LINT tokens in src/ui", () => {
  it("TS003-TOKEN-LINT: the scanner detects a literal colour and a Tailwind numeric size", () => {
    expect(scan('const c = "bg-[#fff]";')).not.toEqual([]);
    expect(scan('const c = "size-8";')).not.toEqual([]);
    expect(scan('const c = "bg-surface-panel p-(--gr-space-4)";')).toEqual([]);
  });

  it("TS003-TOKEN-LINT: the primitives exist, so the lint covers them", () => {
    const names = sources().map((p) => relative(uiDir, p));
    for (const f of [
      "Button.tsx", "IconButton.tsx", "Chip.tsx", "Pill.tsx", "StatusDot.tsx", "RepoBadge.tsx",
      "SegmentedControl.tsx", "Toggle.tsx", "Tree.tsx", "Table.tsx", "Tooltip.tsx", "Menu.tsx",
      "Popover.tsx", "Dialog.tsx", "Toast.tsx", "icons.tsx", "Portal.tsx", "cx.ts", "index.ts",
    ]) {
      expect(names, `src/ui/${f} is missing: the primitive does not exist yet`).toContain(f);
    }
  });

  it("TS003-TOKEN-LINT: no src/ui file has literal colours, measures, radii or theme branches", () => {
    const offenders = sources().flatMap((p) => scan(readFileSync(p, "utf8")).map((f) => `${relative(root, p)} ${f}`));
    expect(offenders, "literal values in src/ui").toEqual([]);
  });
});
