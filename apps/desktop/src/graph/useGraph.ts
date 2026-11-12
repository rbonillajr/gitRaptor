import { skipToken, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { fleetCards, type CardModel } from "@/fleet/fleet-model";
import { useAgentSlots } from "@/fleet/useAgentSlots";
import { useFleet } from "@/fleet/useFleet";
import { useNow } from "@/fleet/useNow";
import type { GraphEntry } from "@/state/engine-cache";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";

export interface GraphInput {
  fleet: RepoFleet | undefined;
  /** The cards of the fleet with the colour slots of `useAgentSlots`: lane and card share name, colour and state. */
  cards: readonly CardModel[];
  /** The last `graph.lanes` result of the open repo, or `null` before the first one. */
  entry: GraphEntry | null;
  /** The clock of the fleet (`useNow`), so the panel and the graph read the same instant with one timer. */
  nowMs: number;
}

/**
 * What the center needs to draw the graph of the open repo. It mounts `useFleet`, so it is the one place that
 * opens the repo; `GraphContent` computes the model with `graphModel` (it needs the conflict view first).
 */
export function useGraph(): GraphInput {
  const { t } = useTranslation();
  const fleet = useFleet().data;
  const selected = useUiStore((s) => s.selectedRepoId);
  const nowMs = useNow();
  const activity = useEngineStore((s) => s.session?.activity ?? false);
  const slots = useAgentSlots();
  const cards = useMemo(() => {
    if (fleet === undefined) return [];
    return fleetCards(fleet, nowMs, activity, t).map((c) =>
      c.agent.sessionId === null ? c : { ...c, agent: { ...c.agent, colorSlot: slots.get(c.agent.sessionId) ?? null } },
    );
  }, [fleet, nowMs, activity, t, slots]);
  const entry = useQuery<GraphEntry>({ queryKey: engineKeys.graph(selected ?? ""), queryFn: skipToken, enabled: selected !== null }).data ?? null;
  return { fleet, cards, entry, nowMs };
}
