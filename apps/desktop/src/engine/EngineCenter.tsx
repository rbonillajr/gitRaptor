import { skipToken, useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { CandidateDto, ConnState } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { ConflictDetailView } from "@/conflicts/ConflictDetailView";
import { ConflictPanel } from "@/conflicts/ConflictPanel";
import { fleetCards } from "@/fleet/fleet-model";
import { FleetView } from "@/fleet/FleetView";
import { useFleet } from "@/fleet/useFleet";
import { useNow } from "@/fleet/useNow";
import { BlockDialog } from "@/guardrails/BlockDialog";
import { blockRows, operationKey, type BlockRow, type CopiedException } from "@/guardrails/blocks-model";
import { GuardrailsView } from "@/guardrails/GuardrailsView";
import { GUARDRAILS_NAV_ID } from "@/guardrails/GuardrailsNavRow";
import { useGuardAlerts } from "@/guardrails/useGuardAlerts";
import { useGuardBlocks } from "@/guardrails/useGuardBlocks";
import type { GlobalEntry } from "@/state/engine-cache";
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

/** What the Guardrails view needs from `EngineCenter`, which owns the dialog and the copied command (D12). */
interface GuardrailsHandlers {
  copied: CopiedException;
  onOpenRow: (row: BlockRow) => void;
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
function FleetContent({ live, copied, onOpenRow }: GuardrailsHandlers & { live: boolean }) {
  const { t } = useTranslation();
  const fleet = useFleet().data;
  const selected = useUiStore((s) => s.selectedRepoId);
  const repoName = useQuery<GlobalEntry>({ queryKey: engineKeys.global, queryFn: skipToken }).data?.repos.find((r) => r.repoId === selected)?.name ?? "";
  const nowMs = useNow();
  const activity = useEngineStore((s) => s.session?.activity ?? false);
  const center = useUiStore((s) => s.center);
  const cards = useMemo(() => (fleet === undefined ? [] : fleetCards(fleet, nowMs, activity, t)), [fleet, nowMs, activity, t]);
  const pairOpen = fleet !== undefined && center.view === "pair" && center.repoId === fleet.repo.repoId ? center : null;
  if (center.view === "guardrails") {
    const back = () => {
      useUiStore.getState().backToMain();
      document.getElementById(GUARDRAILS_NAV_ID)?.focus();
    };
    return <GuardrailsView repoName={repoName} live={live} onBack={back} onOpenRow={onOpenRow} copied={copied} />;
  }
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

function Body({ center, conn, gitVersion, candidates, onRetry, pending = false, children, copied, onOpenRow }: EngineCenterProps & GuardrailsHandlers) {
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
      return children ?? <FleetContent live={conn.kind === "live"} copied={copied} onOpenRow={onOpenRow} />;
  }
}

/**
 * What fills the centre of the window, chosen by `engineView().center`. While the connection is not live the
 * data states carry `data-stale="true"` and the muted text colour; the band explains since when.
 */
export function EngineCenter(props: EngineCenterProps) {
  const { t, i18n } = useTranslation();
  const selected = useUiStore((s) => s.selectedRepoId);
  const guard = useGuardBlocks().data;
  const [dialog, setDialog] = useState<{ id: string; fromToast: boolean } | null>(null);
  const [copy, setCopy] = useState<{ copied: NonNullable<CopiedException>; atMs: number } | null>(null);
  const live = props.conn.kind === "live";
  useGuardAlerts(guard, dialog !== null, (id) => setDialog({ id, fromToast: true }));

  // The copy is forgotten with the repo and once the engine logs how that exception ended (D12).
  const copied =
    copy !== null &&
    copy.copied.repoId === selected &&
    !(guard?.entries ?? []).some((e) => e.kind !== "denial" && e.lastMs >= copy.atMs && operationKey(e.operation) === operationKey(copy.copied.operation))
      ? copy.copied
      : null;
  const dialogRow =
    dialog !== null && guard !== undefined && selected !== null ? blockRows(guard, live, t, i18n.language).find((r) => r.id === dialog.id) : undefined;
  // A dialog whose row left the log (pushed out of the 50 newest) is closed, so it never keeps the block toasts muted.
  const rowGone = dialog !== null && guard !== undefined && selected !== null && dialogRow === undefined;
  useEffect(() => {
    if (rowGone) setDialog(null);
  }, [rowGone]);

  return (
    <div
      data-center={props.center.kind}
      data-stale={props.center.stale ? "true" : undefined}
      className={cx("flex min-h-0 flex-1 flex-col p-(--gr-space-4)", props.center.stale && "text-text-muted")}
    >
      <Body {...props} copied={copied} onOpenRow={(row) => setDialog({ id: row.id, fromToast: false })} />
      {dialog !== null && dialogRow !== undefined && selected !== null ? (
        <BlockDialog
          key={dialog.id}
          row={dialogRow}
          repoId={selected}
          fromToast={dialog.fromToast}
          onClose={() => setDialog(null)}
          onCopied={(c) => setCopy({ copied: c, atMs: Date.now() })}
        />
      ) : null}
    </div>
  );
}
