import type { RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";

export interface ConflictPanelProps {
  fleet: RepoFleet;
  cards: readonly CardModel[];
  nowMs: number;
}

/** Skeleton (T005). */
export function ConflictPanel(props: ConflictPanelProps): null {
  void props;
  return null;
}
