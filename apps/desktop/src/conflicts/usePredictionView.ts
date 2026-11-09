import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";
import { useEngineStore } from "@/state/engine-store";
import { predictionView, type PredictionViewModel } from "./prediction-model";

/** The model the panel paints, with `isNew` resolved from the repo's `AlertState` (D6): the one US-DSK-005 consumes. */
export function usePredictionView(fleet: RepoFleet, cards: readonly CardModel[], nowMs: number): PredictionViewModel {
  const { t } = useTranslation();
  const known = useEngineStore((s) => s.alertState.get(fleet.repo.repoId));
  return predictionView(fleet, cards, nowMs, t, (pairId) => known?.get(pairId) === "alerted");
}
