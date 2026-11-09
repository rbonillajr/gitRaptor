import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactElement, type ReactNode, type RefCallback, type RefObject } from "react";
import { cx } from "./cx";
import { useAnchoredPosition, type Placement } from "./hooks/useAnchoredPosition";
import { useDismiss } from "./hooks/useDismiss";
import { useFadeIn } from "./hooks/useFadeIn";
import { useRovingFocus } from "./hooks/useRovingFocus";
import { useTypeahead } from "./hooks/useTypeahead";
import { Check } from "./icons";
import { Portal } from "./Portal";
import { readPx } from "./tokens";
import { Tooltip } from "./Tooltip";

export type MenuEntry =
  | { type: "item"; id: string; label: string; hint?: string; disabled?: boolean; disabledReason?: string; onSelect: () => void }
  | { type: "radio"; id: string; label: string; checked: boolean; hint?: string; disabled?: boolean; disabledReason?: string; onSelect: () => void }
  | { type: "checkbox"; id: string; label: string; checked: boolean; disabled?: boolean; disabledReason?: string; onCheckedChange: (checked: boolean) => void }
  | { type: "separator"; id: string }
  | { type: "group"; id: string; label: string; entries: ReadonlyArray<MenuEntry> };

type ActionEntry = Exclude<MenuEntry, { type: "separator" } | { type: "group" }>;

export interface MenuTriggerProps {
  ref: RefCallback<HTMLElement>;
  id: string;
  "aria-haspopup": "menu";
  "aria-expanded": boolean;
  "aria-controls": string | undefined;
  onClick: () => void;
  onKeyDown: (e: KeyboardEvent<HTMLElement>) => void;
}

export interface MenuProps {
  /** `aria-label` of the menu. */
  label: string;
  entries: ReadonlyArray<MenuEntry>;
  /** The caller paints the trigger and spreads these props on it. */
  trigger: (props: MenuTriggerProps) => ReactElement;
  placement?: Placement;
}

const ITEM = "[role^='menuitem']";
const ROLE = { item: "menuitem", radio: "menuitemradio", checkbox: "menuitemcheckbox" } as const;

/** Action entries in DOM order (groups flattened). */
function actions(entries: ReadonlyArray<MenuEntry>): ActionEntry[] {
  return entries.flatMap((e) => (e.type === "group" ? actions(e.entries) : e.type === "separator" ? [] : [e]));
}

interface PanelProps {
  label: string;
  entries: ReadonlyArray<MenuEntry>;
  id: string | undefined;
  highlightedId: string | undefined;
  onActivate: ((entry: ActionEntry) => void) | undefined;
  onKeyDown: ((e: KeyboardEvent<HTMLDivElement>) => void) | undefined;
  panelRef: RefObject<HTMLDivElement | null> | undefined;
}

function Indicator({ entry }: { entry: ActionEntry }) {
  if (entry.type === "item") return null;
  const box = "inline-flex size-(--gr-space-4) shrink-0 items-center justify-center border-(length:--gr-size-lane-stroke)";
  if (entry.type === "radio") {
    return (
      <span aria-hidden="true" className={cx(box, "rounded-pill", entry.checked ? "border-accent" : "border-text-muted")}>
        {entry.checked ? <span className="size-(--gr-space-2) rounded-pill bg-accent" /> : null}
      </span>
    );
  }
  return (
    <span aria-hidden="true" className={cx(box, "rounded-xs", entry.checked ? "border-accent bg-accent text-text-on-accent" : "border-text-muted")}>
      {entry.checked ? <Check size="sm" /> : null}
    </span>
  );
}

function Item({ entry, highlighted, onActivate }: { entry: ActionEntry; highlighted: boolean; onActivate: PanelProps["onActivate"] }) {
  const disabled = entry.disabled === true;
  const node = (
    <div
      role={ROLE[entry.type]}
      data-entry-id={entry.id}
      data-highlighted={highlighted ? "" : undefined}
      aria-checked={entry.type === "item" ? undefined : entry.checked}
      aria-disabled={disabled ? "true" : undefined}
      tabIndex={-1}
      onClick={() => {
        if (!disabled) onActivate?.(entry);
      }}
      className={cx(
        "mx-(--gr-space-1) flex h-(--gr-size-target-action) items-center gap-(--gr-space-2) rounded-sm px-(--gr-space-3) cursor-pointer select-none gr-ring-inset gr-motion [font:var(--gr-type-body)]",
        "data-highlighted:bg-surface-hover focus:bg-surface-hover",
        disabled ? "cursor-not-allowed text-text-disabled" : "text-text-main",
      )}
    >
      <Indicator entry={entry} />
      <span className="min-w-0 flex-1 truncate">{entry.label}</span>
      {"hint" in entry && entry.hint ? <span className="shrink-0 text-text-muted [font:var(--gr-type-meta)]">{entry.hint}</span> : null}
    </div>
  );
  return disabled && entry.disabledReason && onActivate ? <Tooltip content={entry.disabledReason}>{node}</Tooltip> : node;
}

function Entries({ entries, highlightedId, onActivate }: { entries: ReadonlyArray<MenuEntry>; highlightedId: string | undefined; onActivate: PanelProps["onActivate"] }): ReactNode {
  return entries.map((entry) => {
    if (entry.type === "separator") return <div key={entry.id} role="separator" className="mx-(--gr-space-1) my-(--gr-space-1) h-px bg-border-subtle" />;
    if (entry.type === "group") return <Group key={entry.id} entry={entry} highlightedId={highlightedId} onActivate={onActivate} />;
    return <Item key={entry.id} entry={entry} highlighted={entry.id === highlightedId} onActivate={onActivate} />;
  });
}

function Group({ entry, highlightedId, onActivate }: { entry: Extract<MenuEntry, { type: "group" }>; highlightedId: string | undefined; onActivate: PanelProps["onActivate"] }) {
  const titleId = useId();
  return (
    <div role="group" aria-labelledby={titleId}>
      <div id={titleId} className="px-(--gr-space-3) text-text-muted [font:var(--gr-type-caption)]">
        {entry.label}
      </div>
      <Entries entries={entry.entries} highlightedId={highlightedId} onActivate={onActivate} />
    </div>
  );
}

function Panel({ label, entries, id, highlightedId, onActivate, onKeyDown, panelRef }: PanelProps) {
  return (
    <div
      ref={panelRef}
      role="menu"
      id={id}
      aria-label={label}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className="flex w-fit min-w-(--gr-size-sidebar) max-w-(--gr-size-detail) flex-col rounded-md bg-surface-raised py-(--gr-space-2) text-text-main shadow-(--gr-shadow-popover)"
    >
      <Entries entries={entries} highlightedId={highlightedId} onActivate={onActivate} />
    </div>
  );
}

/** The menu as it looks, with no behaviour: the catalog paints it (one item can be `highlightedId`). */
export function MenuSurface({ label, entries, highlightedId, id }: { label: string; entries: ReadonlyArray<MenuEntry>; highlightedId?: string; id?: string }) {
  return <Panel label={label} entries={entries} id={id} highlightedId={highlightedId} onActivate={undefined} onKeyDown={undefined} panelRef={undefined} />;
}

interface FloatProps {
  label: string;
  entries: ReadonlyArray<MenuEntry>;
  id: string;
  placement: Placement;
  trigger: RefObject<HTMLElement | null>;
  initial: "first" | "last";
  close: (returnFocus: boolean) => void;
}

/** Mounted inside the Portal: positions itself, takes the focus and owns the keyboard. */
function MenuFloat({ label, entries, id, placement, trigger, initial, close }: FloatProps) {
  const wrap = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const shown = useFadeIn();
  const flat = actions(entries);
  const { onKeyDown: roving, focusIndex } = useRovingFocus(panel, ITEM, { orientation: "vertical", loop: true, skipDisabled: false });
  const typeahead = useTypeahead(() => flat.map((e) => e.label));
  useAnchoredPosition(trigger, wrap, true, { placement, offset: readPx("--gr-space-2"), padding: readPx("--gr-window-margin") });
  useDismiss([wrap, trigger], true, () => close(false), { escape: false });

  useEffect(() => {
    focusIndex(initial === "last" ? flat.length - 1 : 0);
    // Only on opening: later renders must not steal the focus back.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const activate = (entry: ActionEntry) => {
    if (entry.type === "checkbox") {
      entry.onCheckedChange(!entry.checked);
      return;
    }
    entry.onSelect();
    close(true);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close(true);
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      close(true);
      return;
    }
    const itemEl = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>(ITEM) : null;
    const at = flat.findIndex((entry) => entry.id === itemEl?.dataset["entryId"]);
    const entry = flat[at];
    if ((e.key === "Enter" || e.key === " ") && entry) {
      e.preventDefault();
      if (entry.disabled !== true) activate(entry);
      return;
    }
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && e.key !== " ") {
      const hit = typeahead(e.key, at);
      if (hit !== null) focusIndex(hit);
      return;
    }
    roving(e);
  };

  return (
    <div ref={wrap} role="region" aria-label={label} className={cx("fixed z-(--gr-z-popover) gr-motion", shown ? "opacity-100" : "opacity-0")}>
      <Panel label={label} entries={entries} id={id} highlightedId={undefined} onActivate={activate} onKeyDown={onKeyDown} panelRef={panel} />
    </div>
  );
}

/** A menu of actions (APG menu button): the caller paints the trigger, the menu owns focus and keys (D13). */
export function Menu({ label, entries, trigger, placement = "bottom-start" }: MenuProps) {
  const [open, setOpen] = useState<"first" | "last" | null>(null);
  const triggerEl = useRef<HTMLElement | null>(null);
  const triggerId = useId();
  const menuId = useId();

  const close = (returnFocus: boolean) => {
    setOpen(null);
    if (returnFocus) triggerEl.current?.focus();
  };

  return (
    <>
      {trigger({
        ref: (node) => {
          triggerEl.current = node;
        },
        id: triggerId,
        "aria-haspopup": "menu",
        "aria-expanded": open !== null,
        "aria-controls": open !== null ? menuId : undefined,
        onClick: () => setOpen((cur) => (cur === null ? "first" : null)),
        onKeyDown: (e) => {
          if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") {
            e.preventDefault();
            setOpen("first");
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setOpen("last");
          }
        },
      })}
      {open !== null ? (
        <Portal>
          <MenuFloat label={label} entries={entries} id={menuId} placement={placement} trigger={triggerEl} initial={open} close={close} />
        </Portal>
      ) : null}
    </>
  );
}
