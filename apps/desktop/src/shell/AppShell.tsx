import { useQuery } from "@tanstack/react-query";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import { appInfoQuery } from "@/client/queries";
import { useUiStore } from "@/state/ui-store";
import { Panel } from "@/ui/Panel";
import { TitleBar } from "./TitleBar";
import { useLayout } from "./useLayout";
import { useShortcuts } from "./useShortcuts";
import { useTheme } from "./useTheme";

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
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const toggleDetail = useUiStore((s) => s.toggleDetail);
  const cycleTheme = useUiStore((s) => s.cycleTheme);
  const { wide } = useLayout();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const detailButton = useRef<HTMLButtonElement>(null);
  const detail = useRef<HTMLElement>(null);
  const sidebarId = useId();
  const detailId = useId();
  const info = useQuery(appInfoQuery);

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
      setDrawerOpen(false);
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
          heading={t("shell.sidebar.label")}
          hidden={!sidebarOpen}
          className="w-(--gr-size-sidebar) shrink-0"
        />
        <Panel as="main" className="min-w-0 flex-1 items-center justify-center gap-(--gr-space-2) text-center">
          <p className="m-0 text-text-muted">{t("shell.center.empty")}</p>
          {info.data ? (
            <p className="m-0 text-text-faint [font:var(--gr-type-meta)]">{t("shell.center.version", { version: info.data.version })}</p>
          ) : null}
        </Panel>
        <Panel
          as="aside"
          ref={detail}
          id={detailId}
          aria-label={t("shell.detail.label")}
          heading={t("shell.detail.label")}
          hidden={!detailVisible}
          tabIndex={-1}
          onKeyDown={trapFocus}
          className={
            wide
              ? "w-(--gr-size-detail) shrink-0 focus:outline-none"
              : "absolute top-0 right-(--gr-window-margin) bottom-(--gr-window-margin) z-10 w-(--gr-size-detail) shadow-(--gr-shadow-popover) focus:outline-none"
          }
        />
      </div>
    </div>
  );
}
