import {
  cloneElement,
  isValidElement,
  useEffect,
  useId,
  useRef,
  useState,
  type FocusEvent,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type ReactElement,
  type Ref,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import { cx } from "./cx";
import { useAnchoredPosition, type Placement } from "./hooks/useAnchoredPosition";
import { useFadeIn } from "./hooks/useFadeIn";
import { Portal } from "./Portal";
import { mergeRefs } from "./refs";
import { readPx } from "./tokens";

export interface TooltipProps {
  content: string;
  shortcut?: { keys: string; aria: string };
  /** Forces `aria-describedby` on the trigger (an `aria-disabled` trigger gets it anyway). */
  describes?: boolean;
  placement?: Placement;
  /** One focusable element that takes a `ref` and event handlers. */
  children: ReactElement;
}

interface ChildProps {
  ref?: Ref<HTMLElement> | undefined;
  "aria-disabled"?: boolean | "true" | "false" | undefined;
  "aria-describedby"?: string | undefined;
  "aria-keyshortcuts"?: string | undefined;
  onPointerEnter?: ((e: PointerEvent<HTMLElement>) => void) | undefined;
  onPointerLeave?: ((e: PointerEvent<HTMLElement>) => void) | undefined;
  onFocus?: ((e: FocusEvent<HTMLElement>) => void) | undefined;
  onBlur?: ((e: FocusEvent<HTMLElement>) => void) | undefined;
  onClick?: ((e: MouseEvent<HTMLElement>) => void) | undefined;
  onKeyDown?: ((e: KeyboardEvent<HTMLElement>) => void) | undefined;
}

const OPEN_DELAY_MS = 500;
const SKIP_WINDOW_MS = 300;
const GRACE_MS = 100;

/** Time of the last tooltip closing, shared by every Tooltip: a neighbour opened inside 300 ms skips the delay (D12). */
let lastClosedAt = Number.NEGATIVE_INFINITY;

/** The visual of a tooltip with no behaviour: text plus the shortcut in mono (and its spoken phrase). */
export function TooltipSurface({ content, shortcut, id }: { content: string; shortcut?: string; id?: string }) {
  const { t } = useTranslation();
  return (
    <div
      role="tooltip"
      id={id}
      className="max-w-(--gr-size-sidebar) rounded-sm bg-surface-raised px-(--gr-space-2) py-(--gr-space-1) text-text-main shadow-(--gr-shadow-popover) [font:var(--gr-type-secondary)]"
    >
      {content}
      {shortcut ? (
        <>
          <span aria-hidden="true" className="ms-(--gr-space-2) text-text-muted [font:var(--gr-type-mono)]">
            {shortcut}
          </span>
          <span className="sr-only">{t("ui.tooltip.shortcut", { keys: shortcut })}</span>
        </>
      ) : null}
    </div>
  );
}

interface FloatProps {
  anchor: RefObject<HTMLElement | null>;
  placement: Placement;
  content: string;
  shortcut: string | undefined;
  id: string;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
}

/**
 * Mounted inside the Portal so `useAnchoredPosition` finds its element on the first layout. The wrapper is a
 * named region so the floating text sits inside a landmark (axe `region`); it is gone as soon as the tooltip closes.
 */
function TooltipFloat({ anchor, placement, content, shortcut, id, onPointerEnter, onPointerLeave }: FloatProps) {
  const ref = useRef<HTMLDivElement>(null);
  const shown = useFadeIn();
  useAnchoredPosition(anchor, ref, true, { placement, offset: readPx("--gr-space-2"), padding: readPx("--gr-window-margin") });
  return (
    <div
      ref={ref}
      role="region"
      aria-label={content}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      className={cx("fixed z-(--gr-z-tooltip) gr-motion", shown ? "opacity-100" : "opacity-0")}
    >
      <TooltipSurface content={content} id={id} {...(shortcut ? { shortcut } : {})} />
    </div>
  );
}

/**
 * Describes its trigger. Keyboard focus opens it at once, hover after 500 ms (300 ms window between neighbours);
 * Escape, blur, leaving and activating close it. The pointer may travel onto the tooltip (WCAG 1.4.13).
 */
export function Tooltip({ content, shortcut, describes = false, placement = "bottom", children }: TooltipProps) {
  const [open, setOpen] = useState(false);
  const isOpen = useRef(false);
  const id = useId();
  const anchor = useRef<HTMLElement | null>(null);
  const timers = useRef<{ open?: ReturnType<typeof setTimeout>; close?: ReturnType<typeof setTimeout> }>({});

  const clear = () => {
    clearTimeout(timers.current.open);
    clearTimeout(timers.current.close);
  };
  const show = () => {
    clear();
    isOpen.current = true;
    setOpen(true);
  };
  const hide = () => {
    clear();
    if (!isOpen.current) return;
    lastClosedAt = Date.now();
    isOpen.current = false;
    setOpen(false);
  };
  const scheduleShow = () => {
    clear();
    const since = Date.now() - lastClosedAt;
    if (since >= 0 && since < SKIP_WINDOW_MS) show();
    else timers.current.open = setTimeout(show, OPEN_DELAY_MS);
  };
  const scheduleHide = () => {
    clear();
    timers.current.close = setTimeout(hide, GRACE_MS);
  };
  useEffect(() => {
    const pending = timers.current;
    return () => {
      clearTimeout(pending.open);
      clearTimeout(pending.close);
    };
  }, []);

  if (!isValidElement<ChildProps>(children)) return children;
  const own = children.props;
  const disabled = own["aria-disabled"] === true || own["aria-disabled"] === "true";
  const describedBy = open && (describes || disabled) ? [own["aria-describedby"], id].filter(Boolean).join(" ") : own["aria-describedby"];

  return (
    <>
      {cloneElement(children, {
        ref: mergeRefs(own.ref, anchor),
        "aria-describedby": describedBy,
        "aria-keyshortcuts": shortcut?.aria ?? own["aria-keyshortcuts"],
        onPointerEnter: (e: PointerEvent<HTMLElement>) => {
          own.onPointerEnter?.(e);
          scheduleShow();
        },
        onPointerLeave: (e: PointerEvent<HTMLElement>) => {
          own.onPointerLeave?.(e);
          if (isOpen.current) scheduleHide();
          else clear();
        },
        onFocus: (e: FocusEvent<HTMLElement>) => {
          own.onFocus?.(e);
          show();
        },
        onBlur: (e: FocusEvent<HTMLElement>) => {
          own.onBlur?.(e);
          hide();
        },
        onClick: (e: MouseEvent<HTMLElement>) => {
          own.onClick?.(e);
          // An aria-disabled trigger does nothing when activated: its tooltip is the reason, so it stays.
          if (!disabled) hide();
        },
        onKeyDown: (e: KeyboardEvent<HTMLElement>) => {
          own.onKeyDown?.(e);
          if (e.key === "Escape") hide();
        },
      })}
      {open ? (
        <Portal>
          <TooltipFloat
            anchor={anchor}
            placement={placement}
            content={content}
            shortcut={shortcut?.keys}
            id={id}
            onPointerEnter={clear}
            onPointerLeave={scheduleHide}
          />
        </Portal>
      ) : null}
    </>
  );
}
