import { skipToken, useQuery, useQueryClient } from "@tanstack/react-query";
import { useSyncExternalStore, type ReactNode, type Ref } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import type { RepoFleet } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { focusConflictPanel } from "@/conflicts/focus";
import type { EngineView } from "@/engine/engine-view";
import type { GlobalEntry } from "@/state/engine-cache";
import { useUiStore, type Theme } from "@/state/ui-store";
import { Button, IconButton, StatusPill } from "@/ui";

export interface TitleBarProps {
  sidebarId: string;
  detailId: string;
  sidebarOpen: boolean;
  detailOpen: boolean;
  theme: Theme;
  /** The engine connection pill (from `engineView`). */
  pill: EngineView["pill"];
  onToggleSidebar: () => void;
  onToggleDetail: () => void;
  onCycleTheme: () => void;
  /** The right panel button, where focus returns when the drawer closes. */
  detailButtonRef: Ref<HTMLButtonElement>;
}

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

const COMPACT_QUERY = "(max-width: 999px)";
const subscribeCompact = (onChange: () => void) => {
  const list = window.matchMedia(COMPACT_QUERY);
  list.addEventListener("change", onChange);
  return () => list.removeEventListener("change", onChange);
};
const isCompact = () => window.matchMedia(COMPACT_QUERY).matches;

/**
 * The ⚡ pill (D16): the sum of the counted conflicts of every repo, only above zero. It opens the repo with the most
 * (a tie: the open one, then by name) and, if that repo has a single ⚡ pair, its detail; else it focuses the panel.
 */
function ConflictPill() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const compact = useSyncExternalStore(subscribeCompact, isCompact);
  const global = useQuery<GlobalEntry>({ queryKey: engineKeys.global, queryFn: skipToken }).data;
  const counted = (global?.repos ?? []).flatMap((r) =>
    r.attention.conflicts.state === "counted" ? [{ id: r.repoId, name: r.name, count: r.attention.conflicts.count }] : [],
  );
  const total = counted.reduce((n, r) => n + r.count, 0);
  if (total <= 0) return null;

  const open = () => {
    const selected = useUiStore.getState().selectedRepoId;
    const top = [...counted].sort(
      (x, y) => y.count - x.count || Number(y.id === selected) - Number(x.id === selected) || x.name.localeCompare(y.name),
    )[0];
    if (top === undefined) return;
    const bolts = client.getQueryData<RepoFleet>(engineKeys.repo(top.id))?.repo.pairs.filter((p) => p.level === "conflict") ?? [];
    const only = bolts.length === 1 ? bolts[0] : undefined;
    flushSync(() => {
      useUiStore.getState().selectRepo(top.id);
      if (only !== undefined) useUiStore.getState().openPairDetail({ repoId: top.id, pairId: only.pairId });
    });
    if (only === undefined) focusConflictPanel();
  };
  const text = t(compact ? "conflicts.pill.compact" : total === 1 ? "conflicts.pill_one" : "conflicts.pill_other", { count: total }).replace(/^⚡\s*/, "");
  return (
    <Button variant="danger" size="sm" aria-label={t("conflicts.pill.label", { count: total })} onClick={open}>
      <span aria-hidden="true">⚡</span> {text}
    </Button>
  );
}

/**
 * The window title bar: draggable, leaves room for the macOS traffic lights and holds the left panel,
 * theme and right panel buttons (DSF-DSK-001 § 2.2).
 */
export function TitleBar(props: TitleBarProps) {
  const { t } = useTranslation();
  const { pill } = props;
  return (
    <header data-tauri-drag-region className="flex h-(--gr-size-titlebar) shrink-0 items-center gap-(--gr-space-2) pr-(--gr-window-margin) pl-20">
      <IconButton
        label={t("shell.sidebar.toggle")}
        icon={<PanelIcon side="left" />}
        aria-expanded={props.sidebarOpen}
        aria-controls={props.sidebarId}
        onClick={props.onToggleSidebar}
      />
      <StatusPill symbol={pill.symbol} tone={pill.tone} label={t(pill.key, pill.values)} />
      <div data-tauri-drag-region className="flex-1 self-stretch" />
      <IconButton label={t(`shell.theme.toggle.${props.theme}`)} icon={<ThemeIcon theme={props.theme} />} onClick={props.onCycleTheme} />
      <IconButton
        ref={props.detailButtonRef}
        label={t("shell.detail.toggle")}
        icon={<PanelIcon side="right" />}
        aria-expanded={props.detailOpen}
        aria-controls={props.detailId}
        onClick={props.onToggleDetail}
      />
      <ConflictPill />
    </header>
  );
}
