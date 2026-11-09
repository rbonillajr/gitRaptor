import { cx } from "./cx";

type Lane = 1 | 2 | 3 | 4 | 5 | 7;

/** Lane 6 and the base lane are out (text-on-accent contrast, primitives spec C11); the order is part of the hash contract. */
const LANES: readonly Lane[] = [1, 2, 3, 4, 5, 7];

const BG: Record<Lane, string> = {
  1: "bg-lane-1",
  2: "bg-lane-2",
  3: "bg-lane-3",
  4: "bg-lane-4",
  5: "bg-lane-5",
  7: "bg-lane-7",
};

/** FNV-1a (32 bits) over the UTF-16 units of `repoId`, mod 6 over the allowed lanes: stable per repo. */
export function laneFor(repoId: string): Lane {
  let h = 0x811c9dc5;
  for (let i = 0; i < repoId.length; i += 1) {
    h ^= repoId.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return LANES[h % LANES.length] ?? 1;
}

const segmenter = new Intl.Segmenter("es", { granularity: "grapheme" });

/** The repo's initial on its lane colour. Hidden from assistive tech: the repo name is always next to it. */
export function RepoBadge({ repoId, name, className }: { repoId: string; name: string; className?: string }) {
  const first = segmenter.segment(name)[Symbol.iterator]().next().value?.segment ?? "";
  return (
    <span
      aria-hidden="true"
      className={cx(
        "inline-flex size-(--gr-space-5) shrink-0 items-center justify-center rounded-xs text-text-on-accent [font:var(--gr-type-caption)]",
        BG[laneFor(repoId)],
        className,
      )}
    >
      {first.toLocaleUpperCase("es")}
    </span>
  );
}
