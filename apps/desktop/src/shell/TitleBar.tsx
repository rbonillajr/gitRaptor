import type { ReactNode, Ref } from "react";
import { useTranslation } from "react-i18next";
import type { Theme } from "@/state/ui-store";

export interface TitleBarProps {
  sidebarId: string;
  detailId: string;
  sidebarOpen: boolean;
  detailOpen: boolean;
  theme: Theme;
  onToggleSidebar: () => void;
  onToggleDetail: () => void;
  onCycleTheme: () => void;
  /** The right panel button, where focus returns when the drawer closes. */
  detailButtonRef: Ref<HTMLButtonElement>;
}

const BUTTON =
  "inline-flex size-8 items-center justify-center rounded-md bg-surface-control text-text-muted shadow-(--gr-highlight) " +
  "hover:bg-surface-hover hover:text-text-main aria-expanded:text-accent-text " +
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent";

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      {children}
    </svg>
  );
}

const PanelIcon = ({ side }: { side: "left" | "right" }) => (
  <Icon>
    <rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2" />
    <path d={side === "left" ? "M6 3v10" : "M10 3v10"} />
  </Icon>
);

const ThemeIcon = ({ theme }: { theme: Theme }) => (
  <Icon>
    {theme === "light" ? <circle cx="8" cy="8" r="3.25" /> : null}
    {theme === "dark" ? <path d="M12.5 10A5 5 0 0 1 6 3.5a5 5 0 1 0 6.5 6.5Z" /> : null}
    {theme === "auto" ? (
      <>
        <circle cx="8" cy="8" r="5.25" />
        <path d="M8 2.75v10.5A5.25 5.25 0 0 0 8 2.75Z" fill="currentColor" />
      </>
    ) : null}
  </Icon>
);

/**
 * The window title bar: draggable, leaves room for the macOS traffic lights and holds the left panel,
 * theme and right panel buttons (DSF-DSK-001 § 2.2).
 */
export function TitleBar(props: TitleBarProps) {
  const { t } = useTranslation();
  return (
    <header data-tauri-drag-region className="flex h-(--gr-size-titlebar) shrink-0 items-center gap-(--gr-space-2) pr-(--gr-window-margin) pl-20">
      <button
        type="button"
        className={BUTTON}
        aria-label={t("shell.sidebar.toggle")}
        aria-expanded={props.sidebarOpen}
        aria-controls={props.sidebarId}
        onClick={props.onToggleSidebar}
      >
        <PanelIcon side="left" />
      </button>
      <div data-tauri-drag-region className="flex-1 self-stretch" />
      <button type="button" className={BUTTON} aria-label={t(`shell.theme.toggle.${props.theme}`)} onClick={props.onCycleTheme}>
        <ThemeIcon theme={props.theme} />
      </button>
      <button
        ref={props.detailButtonRef}
        type="button"
        className={BUTTON}
        aria-label={t("shell.detail.toggle")}
        aria-expanded={props.detailOpen}
        aria-controls={props.detailId}
        onClick={props.onToggleDetail}
      >
        <PanelIcon side="right" />
      </button>
    </header>
  );
}
