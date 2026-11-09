import type { PairDto } from "@/client/engine-types";

export type AlertState = ReadonlyMap<string, ReadonlyMap<string, "seeded" | "alerted">>;

export type AlertInput =
  | { kind: "snapshot"; pairs: readonly PairDto[]; truncated: boolean; first: boolean }
  | { kind: "changed"; pairs: readonly PairDto[]; removed: readonly string[] }
  | { kind: "closed" };

/** Skeleton: returns the received state and raises nothing (T004). */
export function nextAlerts(
  state: ReadonlyMap<string, "seeded" | "alerted">,
  input: AlertInput,
): { state: Map<string, "seeded" | "alerted">; raised: string[] } {
  void input;
  return { state: new Map(state), raised: [] };
}
