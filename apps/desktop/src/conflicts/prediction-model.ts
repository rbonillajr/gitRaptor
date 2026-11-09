import type { TFunction } from "i18next";
import type { ConfirmCommandDto, LimitCode, RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";

export type SideRef = { kind: "worktree"; key: string } | { kind: "base" };

export interface ConflictBandModel {
  pairId: string;
  level: "conflict" | "overlap";
  a: SideRef;
  b: SideRef;
  aLabel: string;
  bLabel: string;
  firstFile: string | null;
  moreText: string | null;
  freshnessText: string;
  stale: boolean;
  isNew: boolean;
  a11yLabel: string;
}

export interface BasePending {
  base: string;
  repo: string;
  command: ConfirmCommandDto;
}

export interface PredictionViewModel {
  bands: ConflictBandModel[];
  lines: string[];
  basePending: BasePending | null;
  limits: LimitCode[];
}

export interface PairDetailViewModel {
  status: "default" | "overlap-only" | "against-base" | "loading" | "stale" | "calculating" | "pending" | "resolved" | "gone";
  title: string;
  subtitle: string;
  freshnessText: string;
  conflictFiles: Array<{ path: string; kindText: string; aText: string; bText: string; mergeRule: boolean }>;
  overlapFiles: string[];
  truncated: boolean;
  basePending: BasePending | null;
}

/** Skeleton: returns an empty model (T004 builds the rules). */
export function predictionView(
  fleet: RepoFleet,
  cards: readonly CardModel[],
  nowMs: number,
  t: TFunction,
  isNew: (pairId: string) => boolean,
): PredictionViewModel {
  void [fleet, cards, nowMs, t, isNew];
  return { bands: [], lines: [], basePending: null, limits: [] };
}

/** Skeleton: returns an empty model (T004 builds the rules). */
export function pairDetailView(
  fleet: RepoFleet,
  cards: readonly CardModel[],
  pairId: string,
  nowMs: number,
  t: TFunction,
): PairDetailViewModel {
  void [fleet, cards, pairId, nowMs, t];
  return {
    status: "gone",
    title: "",
    subtitle: "",
    freshnessText: "",
    conflictFiles: [],
    overlapFiles: [],
    truncated: false,
    basePending: null,
  };
}
