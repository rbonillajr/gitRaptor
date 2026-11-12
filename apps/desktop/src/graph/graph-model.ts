import type { TFunction } from "i18next";
import type { RepoFleet } from "@/client/engine-types";
import type { CardModel } from "@/fleet/fleet-model";
import type { GraphEntry, GraphModel } from "./graph-types";

interface GraphModelInput {
  fleet: RepoFleet | undefined;
  cards: readonly CardModel[];
  entry: GraphEntry | null;
  t: TFunction;
  locale: string;
  laneA11y?: (key: string) => string | null;
}

/** STUB (T001): the real model is T002. */
export function graphModel(input: GraphModelInput): GraphModel {
  void input;
  throw new Error("not implemented");
}
