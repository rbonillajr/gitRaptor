/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-TOKEN-LINT (TS-DSK-003 T002, D9): prefers-reduced-motion zeroes the single motion token and every
// transition runs on it.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("../src/styles/app.css", import.meta.url), "utf8");
const glass = readFileSync(new URL("../../../packages/design-tokens/css/glass.css", import.meta.url), "utf8");

function body(source: string, head: string): string {
  const start = source.indexOf(head);
  expect(start, `no \`${head}\` in the stylesheet`).not.toBe(-1);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < source.length; i += 1) {
    if (source[i] === "{") depth += 1;
    if (source[i] === "}") depth -= 1;
    if (depth === 0) return source.slice(open + 1, i);
  }
  throw new Error(`unbalanced ${head}`);
}

describe("TS003-TOKEN-LINT motion", () => {
  it("TS003-TOKEN-LINT: app.css zeroes --gr-motion-fast under prefers-reduced-motion", () => {
    const media = body(css, "@media (prefers-reduced-motion: reduce)");
    expect(media.replace(/\s+/g, " "), "reduced-motion rule missing").toMatch(/:root\s*\{\s*--gr-motion-fast:\s*0s;?\s*\}/);
  });

  it("TS003-TOKEN-LINT: gr-motion utilities only use var(--gr-motion-fast) as duration", () => {
    for (const name of ["gr-motion", "gr-motion-transform"]) {
      const b = body(css, `@utility ${name} `);
      const durations = [...b.matchAll(/transition:\s*([^;]+);/g)].flatMap((m) => m[1]!.split(/,(?![^(]*\))/));
      expect(durations.length, `@utility ${name} has no transition`).toBeGreaterThan(0);
      for (const d of durations) expect(d.trim(), `${name} duration`).toMatch(/^[a-z-]+ var\(--gr-motion-fast\)$/);
    }
  });

  it("TS003-TOKEN-LINT: app.css has no @keyframes nor animation", () => {
    expect(css).not.toMatch(/@keyframes|animation\s*:/);
    expect(css, "the reduced-motion rule must exist first").toContain("prefers-reduced-motion");
  });

  it("TS003-TOKEN-LINT: --gr-motion-fast is absent from the bounded [data-theme] blocks", () => {
    const dark = body(glass, '\n[data-theme="dark"] {');
    const light = body(glass, '\n[data-theme="light"] {');
    expect(dark + light).not.toContain("--gr-motion-fast");
  });
});
