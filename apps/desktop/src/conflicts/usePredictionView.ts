import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";
import { predictionView, type PredictionViewModel } from "./prediction-model";

/** Skeleton: no `AlertState` yet, nothing is new (T004). */
export function usePredictionView(fleet: RepoFleet, cards: readonly CardModel[], nowMs: number): PredictionViewModel {
  const { t } = useTranslation();
  return predictionView(fleet, cards, nowMs, t, () => false);
}
