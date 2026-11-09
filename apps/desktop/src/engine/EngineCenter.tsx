import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { CandidateDto, ConnState } from "@/client/engine-types";
import { ConflictDetailView } from "@/conflicts/ConflictDetailView";
import { ConflictPanel } from "@/conflicts/ConflictPanel";
import { fleetCards } from "@/fleet/fleet-model";
import { FleetView } from "@/fleet/FleetView";
import { useFleet } from "@/fleet/useFleet";
import { useNow } from "@/fleet/useNow";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { Button, CopyCommand, Skeleton, StatePanel } from "@/ui";
import { cx } from "@/ui/cx";
import { MIN_GIT, type EngineView } from "./engine-view";

export interface EngineCenterProps {
  center: EngineView["center"];
  conn: ConnState;
  /** `global.engine.gitVersion`, for the a1 check. */
  gitVersion: string | null;
  candidates: readonly CandidateDto[];
  /** `start` is only ever `false` for now: the app cannot start the engine, so it only asks to retry the connection. */
  onRetry: (start: boolean) => void;
  /** Buttons stay `aria-disabled` until the next connection state arrives. */
  pending?: boolean;
  /** Overrides the content state; by default it is the `FleetView` of the open repo. */
  children?: ReactNode;
}

type Incompatible = Extract<ConnState, { kind: "incompatible" }>;

/** Untrusted strings (`version`, `path`) only ever reach the screen as React text, through interpolation. */
function IncompatiblePanel({ conn, onRetry, pending }: { conn: Incompatible; onRetry: (start: boolean) => void; pending: boolean }) {
  const { t } = useTranslation();
  const title = t("engine.incompatible.title");

  if (conn.which === "app-older") {
    return <StatePanel tone="error" title={title} message={t("engine.incompatible.appOlder", { version: conn.daemonVersion ?? "?" })} />;
  }
  if (conn.action === "start") {
    return (
      <StatePanel
        tone="error"
        title={title}
        message={
          <>
            <p className="m-0">{t("engine.incompatible.ownEngine")}</p>
          </>
        }
        action={
          <Button disabled={pending} onClick={() => onRetry(true)}>
            {t("engine.action.start")}
          </Button>
        }
      />
    );
  }
  const text =
    conn.daemonVersion !== null && conn.daemonPath !== null
      ? t("engine.incompatible.daemonOlder", { version: conn.daemonVersion, path: conn.daemonPath })
      : t("engine.incompatible.daemonOlderUnknown");
  return (
    <StatePanel
      tone="error"
      title={title}
      message={
        <>
          <p className="m-0">{text}</p>
          {conn.stopCommand !== null ? (
            <CopyCommand command={conn.stopCommand} />
          ) : conn.action === "copy-stop" ? (
            <p className="m-0">{t("engine.incompatible.noCommand")}</p>
          ) : null}
        </>
      }
    />
  );
}

function NoRepos({ gitVersion, candidates }: { gitVersion: string | null; candidates: readonly CandidateDto[] }) {
  const { t, i18n } = useTranslation();
  const addCommand = i18n.language.startsWith("es") ? "raptor repo add <ruta>" : "raptor repo add <path>";
  const steps = ["step1", "step2", "step3"] as const;
  return (
    <div className="mx-auto flex w-full max-w-(--gr-size-dialog-width) flex-col gap-(--gr-space-4)">
      <h2 className="m-0 text-text-main [font:var(--gr-type-title)]">{t("empty.noRepos.headline")}</h2>
      <ol className="m-0 flex list-none flex-col gap-(--gr-space-3) p-0">
        {steps.map((step) => (
          <li key={step}>
            <h3 className="m-0 text-text-main [font:var(--gr-type-body-strong)]">{t(`empty.noRepos.${step}.title`)}</h3>
            <p className="m-0 text-text-muted [font:var(--gr-type-body)]">{t(`empty.noRepos.${step}.body`)}</p>
          </li>
        ))}
      </ol>
      <p className="m-0 text-text-muted [font:var(--gr-type-body)]">{t("empty.noRepos.privacy")}</p>
      <ul className="m-0 flex list-none flex-col gap-(--gr-space-1) p-0 text-text-main [font:var(--gr-type-body)]">
        <li>
          <span aria-hidden="true" className="text-success-text">
            ✓{" "}
          </span>
          {t("empty.noRepos.check.engine")}
        </li>
        {gitVersion !== null ? (
          <li>
            <span aria-hidden="true" className="text-success-text">
              ✓{" "}
            </span>
            {t("empty.noRepos.check.git", { version: gitVersion })}
          </li>
        ) : null}
      </ul>
      <p className="m-0 text-text-main [font:var(--gr-type-body)]">
        {t("empty.noRepos.howTo")}{" "}
        <code className="rounded-md bg-surface-control px-(--gr-space-2) py-(--gr-space-1) [font:var(--gr-type-mono)]">{addCommand}</code>
      </p>
      {candidates.length > 0 ? (
        <section aria-labelledby="gr-engine-discovered" className="flex flex-col gap-(--gr-space-2)">
          <h3 id="gr-engine-discovered" className="m-0 text-text-main [font:var(--gr-type-body-strong)]">
            {t("empty.noRepos.discovered.title")}
          </h3>
          <ul className="m-0 flex list-none flex-col gap-(--gr-space-1) p-0">
            {candidates.map((c) => (
              <li key={c.path} className="flex flex-col">
                <span className="text-text-main [font:var(--gr-type-body)]">{c.name}</span>
                <span className="break-all text-text-muted [font:var(--gr-type-mono)]">{c.path}</span>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </div>
  );
}

/**
 * The only place that mounts `useFleet`: it opens the default repo and reads its entry. Only for the content state.
 * The conflict panel sits above the main view (D5); a `pair` destination of the open repo replaces the main view (D7).
 */
function FleetContent() {
  const { t } = useTranslation();
  const fleet = useFleet().data;
  const nowMs = useNow();
  const activity = useEngineStore((s) => s.session?.activity ?? false);
  const center = useUiStore((s) => s.center);
  const cards = useMemo(() => (fleet === undefined ? [] : fleetCards(fleet, nowMs, activity, t)), [fleet, nowMs, activity, t]);
  const pairOpen = fleet !== undefined && center.view === "pair" && center.repoId === fleet.repo.repoId ? center : null;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-(--gr-space-3)">
      {fleet !== undefined ? <ConflictPanel fleet={fleet} cards={cards} nowMs={nowMs} /> : null}
      {fleet !== undefined && pairOpen !== null ? (
        <ConflictDetailView key={pairOpen.pairId} fleet={fleet} cards={cards} pairId={pairOpen.pairId} nowMs={nowMs} />
      ) : (
        <FleetView fleet={fleet} nowMs={nowMs} activity={activity} />
      )}
    </div>
  );
}

function Body({ center, conn, gitVersion, candidates, onRetry, pending = false, children }: EngineCenterProps) {
  const { t } = useTranslation();
  switch (center.kind) {
    case "skeleton":
      return <Skeleton />;
    case "stopped":
      return (
        <StatePanel
          tone="error"
          title={t("engine.stopped.title")}
          message={
            <>
            </>
          }
          action={
            <Button disabled={pending} onClick={() => onRetry(true)}>
              {t("engine.action.start")}
            </Button>
          }
        />
      );
    case "error":
      if (conn.kind === "incompatible") return <IncompatiblePanel conn={conn} onRetry={onRetry} pending={pending} />;
      if (conn.kind === "rejected") return <StatePanel tone="error" title={t("engine.rejected.title")} message={t("engine.rejected.message")} />;
      return <StatePanel tone="error" title={t("engine.unsupported.title")} message={t("engine.unsupported.message")} />;
    case "restricted":
      return <StatePanel title={t("engine.restricted.title")} message={t("engine.restricted.message")} />;
    case "waiting-for-git":
      return <StatePanel title={t("empty.waitingForGit.title")} message={t("empty.waitingForGit.message", { min: MIN_GIT })} />;
    case "no-repos":
      return <NoRepos gitVersion={gitVersion} candidates={candidates} />;
    case "content":
      return children ?? <FleetContent />;
  }
}

/**
 * What fills the centre of the window, chosen by `engineView().center`. While the connection is not live the
 * data states carry `data-stale="true"` and the muted text colour; the band explains since when.
 */
export function EngineCenter(props: EngineCenterProps) {
  return (
    <div
      data-center={props.center.kind}
      data-stale={props.center.stale ? "true" : undefined}
      className={cx("flex min-h-0 flex-1 flex-col p-(--gr-space-4)", props.center.stale && "text-text-muted")}
    >
      <Body {...props} />
    </div>
  );
}
