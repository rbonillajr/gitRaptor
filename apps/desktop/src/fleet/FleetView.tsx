import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RepoFleet, SessionDto } from "@/client/engine-types";
import { useEngineStore } from "@/state/engine-store";
import { CardSkeleton, Panel, WorktreeCard } from "@/ui";
import { assignSlots } from "./agent-colors";
import { fleetCards, formatAge } from "./fleet-model";

export interface FleetViewProps {
  /** `undefined` until the repo snapshot is in: the view shows skeletons, never "no agents". */
  fleet: RepoFleet | undefined;
  /** From `useNow`. */
  nowMs: number;
  /** `SessionInfo.activity`. */
  activity: boolean;
}

const MAX_TIMING_ENTRIES = 1000;

const sameSlots = (a: ReadonlyMap<string, number>, b: ReadonlyMap<string, number>) =>
  a.size === b.size && [...a].every(([k, v]) => b.get(k) === v);

/**
 * After the React commit that applied a batch with repo messages, in the first animation frame: one
 * `gr:fleet-delta` measure from the batch's `receivedAt` and one `gr:fleet-delta-painted` mark (NFR-13.1).
 * A batch already in the store when the view mounts was applied before it and is not measured.
 */
function useFleetDeltaMarks(): void {
  const batch = useEngineStore((s) => s.lastRepoBatch);
  const measured = useRef(useEngineStore.getState().lastRepoBatch);
  useEffect(() => {
    if (batch === null || batch === measured.current) return;
    const frame = requestAnimationFrame(() => {
      measured.current = batch;
      performance.measure("gr:fleet-delta", { start: batch.receivedAt });
      performance.mark("gr:fleet-delta-painted");
      // The platform never clears the User Timing buffer: cap it.
      if (performance.getEntriesByName("gr:fleet-delta").length > MAX_TIMING_ENTRIES) {
        performance.clearMeasures("gr:fleet-delta");
        performance.clearMarks("gr:fleet-delta-painted");
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [batch]);
}

/**
 * The fleet of the open repo: one card per worktree in the attention order. Presentational: the data and the
 * clock come in as props. The slot registry and the announcement baseline live in the keyed body, so they
 * reset when the repo changes.
 */
export function FleetView({ fleet, nowMs, activity }: FleetViewProps) {
  const { t } = useTranslation();
  useFleetDeltaMarks();
  if (fleet === undefined) {
    return (
      <Panel as="section" aria-busy="true" data-fleet="loading" className="gap-(--gr-space-2)">
        <div role="status" aria-busy="true" className="flex flex-col gap-(--gr-space-2)">
          <span className="sr-only">{t("fleet.loading")}</span>
          {[0, 1, 2].map((i) => (
            <CardSkeleton key={i} />
          ))}
        </div>
      </Panel>
    );
  }
  return <FleetBody key={fleet.repo.repoId} fleet={fleet} nowMs={nowMs} activity={activity} />;
}

function FleetBody({ fleet, nowMs, activity }: FleetViewProps & { fleet: RepoFleet }) {
  const { t } = useTranslation();
  const { repo } = fleet;
  const cards = useMemo(() => fleetCards(fleet, nowMs, activity, t), [fleet, nowMs, activity, t]);

  const present = [...fleet.sessions.values()].filter((s: SessionDto) => s.state !== "ended");
  const [slots, setSlots] = useState<ReadonlyMap<string, number>>(new Map());
  const nextSlots = assignSlots(slots, present);
  if (!sameSlots(slots, nextSlots)) setSlots(nextSlots);

  const attentionKeys = cards.filter((c) => c.group === "attention").map((c) => c.key);
  const attentionSignature = attentionKeys.join("|");
  const seen = useRef<ReadonlySet<string> | null>(null);
  const [announcement, setAnnouncement] = useState("");
  useEffect(() => {
    const keys = attentionSignature === "" ? [] : attentionSignature.split("|");
    if (seen.current !== null) {
      const entered = keys.filter((k) => !seen.current?.has(k));
      if (entered.length > 0) {
        const names = entered.map((k) => repo.worktrees.find((w) => w.key === k)?.folder ?? k);
        setAnnouncement(names.map((worktree) => t("fleet.attention.announce", { worktree })).join(". "));
      }
    }
    seen.current = new Set(keys);
  }, [attentionSignature, repo.worktrees, t]);

  const base = repo.base.name ?? "base";
  const baseMissing = repo.worktrees.some((w) => w.status.state === "ready" && w.status.divergence.state === "base-missing");
  const title = baseMissing
    ? t("fleet.titleBaseNotFound", { base })
    : repo.fetchedUtcMs === null
      ? t("fleet.titleNeverFetched", { base })
      : t("fleet.title", { base, age: formatAge(nowMs - repo.fetchedUtcMs, t) });
  const noAgents = fleet.detection === true && cards.every((c) => c.agentState === "none");

  return (
    <Panel as="section" heading={title} data-fleet="ready" className="gap-(--gr-space-3)">
      <div role="status" className="sr-only">
        {announcement}
      </div>
      {noAgents ? <p className="m-0 text-text-muted">{t("fleet.empty.agents", { repo: repo.name })}</p> : null}
      <ul aria-label={t("fleet.list.label", { repo: repo.name })} className="m-0 flex list-none flex-col gap-(--gr-space-2) p-0">
        {cards.map((c) => (
          <li key={c.key}>
            <WorktreeCard
              symbol={c.symbol}
              agentLabel={c.agent.label}
              colorSlot={c.agent.sessionId === null ? null : (nextSlots.get(c.agent.sessionId) ?? null)}
              muted={c.agent.muted}
              shared={c.agent.shared}
              stateText={c.stateText}
              main={c.main}
              branchText={c.branchText}
              branchCommit={c.branchCommit}
              filesText={c.filesText}
              syncText={c.syncText}
              activityText={c.activityText}
              attentionText={c.attention?.text ?? null}
              unavailableText={c.unavailableText}
              a11yLabel={c.a11yLabel}
            />
          </li>
        ))}
      </ul>
    </Panel>
  );
}
