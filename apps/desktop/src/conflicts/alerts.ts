import type { PairDto } from "@/client/engine-types";

/** The conflicts each repo already knows: `seeded` ones were there when the repo opened, `alerted` ones raised a toast. */
export type AlertState = ReadonlyMap<string, ReadonlyMap<string, "seeded" | "alerted">>;

export type AlertInput =
  | { kind: "snapshot"; pairs: readonly PairDto[]; truncated: boolean; first: boolean }
  | { kind: "changed"; pairs: readonly PairDto[]; removed: readonly string[] }
  | { kind: "closed" };

const isConflict = (p: PairDto) => p.level === "conflict" && p.state === "current";

/**
 * Which conflicts of one repo are new (DS-US-DSK-008 D12; A5, E1, E2 of DS-US-CKP-008). A new one is a pair that
 * becomes a `current` conflict and is not in `state`; the first snapshot only seeds. A `current` pair without a
 * conflict, `removed`, or absence from a complete snapshot forget a pair; `recalculating` and a truncated snapshot keep it.
 */
export function nextAlerts(
  state: ReadonlyMap<string, "seeded" | "alerted">,
  input: AlertInput,
): { state: Map<string, "seeded" | "alerted">; raised: string[] } {
  if (input.kind === "closed") return { state: new Map(), raised: [] };
  const next = new Map(state);
  const raised: string[] = [];

  const visit = (p: PairDto, seed: boolean) => {
    if (next.has(p.pairId)) {
      if (p.state === "current" && p.level !== "conflict") next.delete(p.pairId);
    } else if (isConflict(p)) {
      next.set(p.pairId, seed ? "seeded" : "alerted");
      if (!seed) raised.push(p.pairId);
    }
  };

  if (input.kind === "snapshot") {
    if (!input.truncated) {
      const listed = new Set(input.pairs.map((p) => p.pairId));
      for (const id of state.keys()) if (!listed.has(id)) next.delete(id);
    }
    for (const p of input.pairs) visit(p, input.first);
  } else {
    const removed = new Set(input.removed);
    for (const id of removed) next.delete(id);
    for (const p of input.pairs) if (!removed.has(p.pairId)) visit(p, false);
  }
  return { state: next, raised };
}
