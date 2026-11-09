import { useEffect, useId, useRef, useState, type ReactElement, type ReactNode, type RefCallback, type RefObject } from "react";
import { cx } from "./cx";
import { useAnchoredPosition, type Placement } from "./hooks/useAnchoredPosition";
import { useDismiss } from "./hooks/useDismiss";
import { useFadeIn } from "./hooks/useFadeIn";
import { Portal } from "./Portal";
import { readPx } from "./tokens";

export interface PopoverTriggerProps {
  ref: RefCallback<HTMLElement>;
  "aria-haspopup": "dialog";
  "aria-expanded": boolean;
  "aria-controls": string | undefined;
  onClick: () => void;
}

export interface PopoverProps {
  /** `aria-label` of the `role="dialog"`. */
  label: string;
  trigger: (props: PopoverTriggerProps) => ReactElement;
  children: ReactNode;
  placement?: Placement;
  /** Controlled mode; without it the popover keeps its own state. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

const FIRST_FOCUSABLE = "button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";

/** The popover as it looks, with no behaviour (the catalog paints it). */
export function PopoverSurface({ label, children, id }: { label: string; children: ReactNode; id?: string }) {
  return (
    <div
      role="dialog"
      id={id}
      aria-label={label}
      className="flex w-(--gr-size-sidebar) flex-col gap-(--gr-space-1) rounded-md bg-surface-raised p-(--gr-space-3) text-text-main shadow-(--gr-shadow-popover)"
    >
      {children}
    </div>
  );
}

interface FloatProps {
  label: string;
  id: string;
  placement: Placement;
  trigger: RefObject<HTMLElement | null>;
  close: (returnFocus: boolean) => void;
  children: ReactNode;
}

/** Mounted inside the Portal: positions itself, moves the focus in and closes on Escape or an outside click. */
function PopoverFloat({ label, id, placement, trigger, close, children }: FloatProps) {
  const ref = useRef<HTMLDivElement>(null);
  const shown = useFadeIn();
  useAnchoredPosition(trigger, ref, true, { placement, offset: readPx("--gr-space-2"), padding: readPx("--gr-window-margin") });
  useDismiss([ref, trigger], true, (reason) => close(reason === "escape"), { focusOutside: true });

  useEffect(() => {
    const target = ref.current?.querySelector<HTMLElement>(FIRST_FOCUSABLE) ?? ref.current?.firstElementChild;
    if (target instanceof HTMLElement) target.focus();
  }, []);

  return (
    <div ref={ref} className={cx("fixed z-(--gr-z-popover) gr-motion", shown ? "opacity-100" : "opacity-0")}>
      <PopoverSurface label={label} id={id}>
        {children}
      </PopoverSurface>
    </div>
  );
}

/** A non-modal panel with controls: no focus trap, Escape returns the focus to the trigger, an outside click does not. */
export function Popover({ label, trigger, children, placement = "bottom-end", open, onOpenChange }: PopoverProps) {
  const [inner, setInner] = useState(false);
  const isOpen = open ?? inner;
  const triggerEl = useRef<HTMLElement | null>(null);
  const id = useId();

  const set = (next: boolean) => {
    if (open === undefined) setInner(next);
    onOpenChange?.(next);
  };
  const close = (returnFocus: boolean) => {
    set(false);
    if (returnFocus) triggerEl.current?.focus();
  };

  return (
    <>
      {trigger({
        ref: (node) => {
          triggerEl.current = node;
        },
        "aria-haspopup": "dialog",
        "aria-expanded": isOpen,
        "aria-controls": isOpen ? id : undefined,
        onClick: () => set(!isOpen),
      })}
      {isOpen ? (
        <Portal>
          <PopoverFloat label={label} id={id} placement={placement} trigger={triggerEl} close={close}>
            {children}
          </PopoverFloat>
        </Portal>
      ) : null}
    </>
  );
}
