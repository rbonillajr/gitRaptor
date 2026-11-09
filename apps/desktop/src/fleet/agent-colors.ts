import type { SessionDto } from "@/client/engine-types";

/** `glass.lane.1` … `.7`. */
export const SLOTS = 7;

/**
 * D7: each present session keeps its slot while it stays present; a new one takes the lowest free slot in
 * `(startedUtcMs, sessionId)` order. With more than 7 present, the extra ones repeat slot 7.
 * Pure: the caller keeps the previous map (a `useRef`) and empties it when the repo changes.
 */
export function assignSlots(previous: ReadonlyMap<string, number>, present: readonly SessionDto[]): Map<string, number> {
  const next = new Map<string, number>();
  const used = new Set<number>();
  for (const s of present) {
    const kept = previous.get(s.sessionId);
    if (kept !== undefined) {
      next.set(s.sessionId, kept);
      used.add(kept);
    }
  }
  const fresh = present
    .filter((s) => !next.has(s.sessionId))
    .sort((a, b) => a.startedUtcMs - b.startedUtcMs || (a.sessionId < b.sessionId ? -1 : a.sessionId > b.sessionId ? 1 : 0));
  for (const s of fresh) {
    let slot = 1;
    while (slot < SLOTS && used.has(slot)) slot += 1;
    next.set(s.sessionId, slot);
    used.add(slot);
  }
  return next;
}
