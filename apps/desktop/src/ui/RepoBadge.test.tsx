/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-BASICS (TS-DSK-003 T004): RepoBadge lane (FNV-1a over UTF-16 units, % 6 over [1,2,3,4,5,7]), initial, aria-hidden.
import { cleanup, render } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, describe, expect, it } from "vitest";
import "@/i18n";
import { laneFor, RepoBadge } from "./RepoBadge";

const LANES = [1, 2, 3, 4, 5, 7];
function fnv(id: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < id.length; i += 1) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}
afterEach(cleanup);

describe("TS003-BASICS RepoBadge", () => {
  it("TS003-BASICS: laneFor follows the FNV-1a rule of the Dev Spec", () => {
    for (const id of ["repo-a", "gitraptor", "ñandú", "", "x".repeat(40)]) {
      expect(laneFor(id), `lane of ${JSON.stringify(id)}`).toBe(LANES[fnv(id) % 6]);
    }
  });

  it("TS003-BASICS: laneFor is deterministic, never 6, and uses all six lanes over 1000 ids", () => {
    const seen = new Set<number>();
    for (let i = 0; i < 1000; i += 1) {
      const id = `repo-${i}-${(i * 2654435761) % 99991}`;
      const lane = laneFor(id);
      expect(laneFor(id), "same id, same lane").toBe(lane);
      expect(lane, "lane 6 is reserved").not.toBe(6);
      seen.add(lane);
    }
    expect([...seen].sort(), "all six lanes in use").toEqual(LANES);
  });

  it("TS003-BASICS: the initial is the first grapheme uppercased; the badge is aria-hidden", () => {
    const { container } = render(<RepoBadge repoId="r1" name="ñandú" />);
    const badge = container.firstElementChild!;
    expect(badge.textContent, "initial of 'ñandú'").toBe("Ñ");
    expect(badge).toHaveAttribute("aria-hidden", "true");
  });

  it("TS003-BASICS: the initial keeps a multi-unit grapheme whole", () => {
    const { container } = render(<RepoBadge repoId="r2" name="🦖 raptor" />);
    expect(container.firstElementChild!.textContent).toBe("🦖");
  });

  it("TS003-BASICS: no axe violations", async () => {
    const { container } = render(<div><RepoBadge repoId="a" name="alpha" /><RepoBadge repoId="b" name="beta" /></div>);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
