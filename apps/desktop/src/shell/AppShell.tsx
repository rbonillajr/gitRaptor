import { skipToken, useQuery } from "@tanstack/react-query";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import { call } from "@/client/ipc";
import { engineKeys } from "@/client/queries";
import { FleetView } from "@/fleet/FleetView";
import { useNow } from "@/fleet/useNow";
import { RepoTreeContainer } from "@/fleet/RepoTreeContainer";
import { EngineBand } from "@/engine/EngineBand";
import { EngineCenter } from "@/engine/EngineCenter";
import { useEngine } from "@/engine/useEngine";
import { GuardrailsNavRow } from "@/guardrails/GuardrailsNavRow";
import { useGuardUnseen } from "@/guardrails/unseen-store";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { Panel } from "@/ui/Panel";
import { TitleBar } from "./TitleBar";
import { useLayout } from "./useLayout";
import { useShortcuts } from "./useShortcuts";
import { useTheme } from "./useTheme";

/**
 * The fleet of the open repo in the detail (DS-US-DSK-005 D15). It only reads the cache entry: `GraphContent` in the
 * center opens the repo and is the one writer of the colour slots.
 */
function AgentStatus() {
  const selected = useUiStore((s) => s.selectedRepoId);
  const fleet = useQuery<RepoFleet>({ queryKey: engineKeys.repo(selected ?? ""), queryFn: skipToken, enabled: selected !== null }).data;
  const nowMs = useNow();
  const activity = useEngineStore((s) => s.session?.activity ?? false);
  if (selected === null) return null;
  return <FleetView fleet={fleet} nowMs={nowMs} activity={activity} />;
}

const FOCUSABLE = "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])";

/**
 * The empty B1 window (DSF-DSK-001 § 2.2 and § 2.4): title bar, sidebar, center and detail. From
 * 1360 px the detail is a third panel that follows the stored preference; below, it is an overlay
 * drawer that starts closed, traps Tab and closes with Esc. The drawer state is not persisted.
 */
export function AppShell() {
  const { t } = useTranslation();
  const theme = useUiStore((s) => s.theme);
  const sidebarOpen = useUiStore((s) => s.sidebarOpen);
  const detailOpen = useUiStore((s) => s.detailOpen);
  const repoOpen = useUiStore((s) => s.selectedRepoId !== null);
  const guardrailsOpen = useUiStore((s) => s.center.view === "guardrails");
  const openGuardrails = useUiStore((s) => s.openGuardrails);
  const unseenBlocks = useGuardUnseen((s) => s.count);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const toggleDetail = useUiStore((s) => s.toggleDetail);
  const cycleTheme = useUiStore((s) => s.cycleTheme);
  const { wide } = useLayout();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const detailButton = useRef<HTMLButtonElement>(null);
  const detail = useRef<HTMLElement>(null);
  const sidebarId = useId();
  const detailId = useId();
  const engine = useEngine();
  // A retry stays pending until the next connection state arrives (a new `conn` object, NFR-13.7).
  const [retriedAt, setRetriedAt] = useState<unknown>(null);
  const retryPending = retriedAt === engine.conn;

  function onRetry(start: boolean) {
    setRetriedAt(engine.conn);
    call("engine_retry", { start }).catch(() => setRetriedAt(null));
  }

  useTheme(theme);

  useEffect(() => {
    const frame = requestAnimationFrame(() => performance.mark("gr:shell-painted"));
    return () => cancelAnimationFrame(frame);
  }, []);

  const detailVisible = wide ? detailOpen : drawerOpen;

  function onToggleDetail() {
    if (wide) {
      toggleDetail();
    } else if (drawerOpen) {
      closeDrawer();
    } else {
      flushSync(() => setDrawerOpen(true));
      detail.current?.focus();
    }
  }

  function closeDrawer() {
    if (wide || !drawerOpen) return;
    setDrawerOpen(false);
    detailButton.current?.focus();
  }

  useShortcuts({ onToggleSidebar: toggleSidebar, onToggleDetail, onEscape: closeDrawer });

  /** Keeps Tab inside the open drawer. */
  function trapFocus(event: KeyboardEvent<HTMLElement>) {
    const drawer = detail.current;
    if (wide || event.key !== "Tab" || !drawer) return;
    const items = Array.from(drawer.querySelectorAll<HTMLElement>(FOCUSABLE));
    const first = items[0] ?? drawer;
    const last = items[items.length - 1] ?? drawer;
    const active = document.activeElement;
    if (event.shiftKey && (active === first || active === drawer)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="flex h-full flex-col bg-surface-window text-text-main [font:var(--gr-type-body)]">
      <TitleBar
        sidebarId={sidebarId}
        detailId={detailId}
        sidebarOpen={sidebarOpen}
        detailOpen={detailVisible}
        theme={theme}
        pill={engine.view.pill}
        onToggleSidebar={toggleSidebar}
        onToggleDetail={onToggleDetail}
        onCycleTheme={cycleTheme}
        detailButtonRef={detailButton}
      />
      <div className="relative flex min-h-0 flex-1 gap-(--gr-gutter) px-(--gr-window-margin) pb-(--gr-window-margin)">
        <Panel
          as="nav"
          id={sidebarId}
          aria-label={t("shell.sidebar.label")}
          hidden={!sidebarOpen}
          className="w-(--gr-size-sidebar) shrink-0 p-0!"
        >
          <RepoTreeContainer />
          {repoOpen && engine.view.center.kind !== "restricted" ? (
            <div className="mt-auto p-(--gr-space-2)">
              <GuardrailsNavRow unseen={unseenBlocks} active={guardrailsOpen} onOpen={openGuardrails} />
            </div>
          ) : null}
        </Panel>
        <Panel as="main" className="min-w-0 flex-1 overflow-hidden p-0!">
          {engine.view.band ? <EngineBand band={engine.view.band} onRetry={onRetry} pending={retryPending} /> : null}
          <EngineCenter
            center={engine.view.center}
            conn={engine.conn}
            gitVersion={engine.gitVersion}
            candidates={engine.candidates}
            onRetry={onRetry}
            pending={retryPending}
          />
        </Panel>
        <Panel
          as="aside"
          ref={detail}
          id={detailId}
          aria-label={t("detail.agents.title")}
          heading={t("detail.agents.title")}
          hidden={!detailVisible}
          tabIndex={-1}
          onKeyDown={trapFocus}
          className={
            wide
              ? "w-(--gr-size-detail) shrink-0 overflow-y-auto focus:outline-none"
              : "absolute top-0 right-(--gr-window-margin) bottom-(--gr-window-margin) z-(--gr-z-drawer) w-(--gr-size-detail) overflow-y-auto shadow-(--gr-shadow-popover) focus:outline-none"
          }
        >
          <AgentStatus />
        </Panel>
      </div>
    </div>
  );
}
