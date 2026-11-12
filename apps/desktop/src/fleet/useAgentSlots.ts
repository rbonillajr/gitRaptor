import { useEffect } from "react";
import { create } from "zustand";
import type { RepoFleet } from "@/client/engine-types";
import { assignSlots } from "./agent-colors";

interface SlotsState {
  repoId: string | null;
  slots: ReadonlyMap<string, number>;
}

/** Not persisted: the colours of the agents live as long as the run and are forgotten with the repo (DS-US-DSK-005 D6). */
const useSlotsStore = create<SlotsState>()(() => ({ repoId: null, slots: new Map() }));

const sameSlots = (a: ReadonlyMap<string, number>, b: ReadonlyMap<string, number>) => a.size === b.size && [...a].every(([k, v]) => b.get(k) === v);

/**
 * The only writer of the colour slots: keeps each present session in its slot, gives a new one the lowest free
 * slot and empties the registry when the repo changes. Mount it in one place (`GraphContent`): with two writers
 * an agent would change colour when the detail drawer opens.
 */
export function useAgentSlotsSync(fleet: RepoFleet | undefined): void {
  const repoId = fleet?.repo.repoId ?? null;
  const sessions = fleet?.sessions;
  useEffect(() => {
    if (repoId === null || sessions === undefined) return;
    const current = useSlotsStore.getState();
    const previous = current.repoId === repoId ? current.slots : new Map<string, number>();
    const next = assignSlots(previous, [...sessions.values()].filter((s) => s.state !== "ended"));
    if (current.repoId !== repoId || !sameSlots(previous, next)) useSlotsStore.setState({ repoId, slots: next });
  }, [repoId, sessions]);
}

/** The colour slot (1 to 7) of a session, or `null` if it has none. */
export function useAgentSlot(sessionId: string): number | null {
  return useSlotsStore((s) => s.slots.get(sessionId) ?? null);
}

/** The slot of every session, for a caller that cannot call a hook per item. */
export function useAgentSlots(): ReadonlyMap<string, number> {
  return useSlotsStore((s) => s.slots);
}
