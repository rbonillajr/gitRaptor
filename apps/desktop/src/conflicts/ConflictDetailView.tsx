import type { RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";

export interface ConflictDetailViewProps {
  fleet: RepoFleet;
  cards: readonly CardModel[];
  pairId: string;
  nowMs: number;
}

/** Skeleton (T006). */
export function ConflictDetailView(props: ConflictDetailViewProps): null {
  void props;
  return null;
}
