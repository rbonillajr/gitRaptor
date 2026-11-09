import { useEffect, useId, useRef, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";
import { cx } from "./cx";
import { useDismiss } from "./hooks/useDismiss";
import { useFadeIn } from "./hooks/useFadeIn";
import { useFocusTrap } from "./hooks/useFocusTrap";
import { Close } from "./icons";
import { IconButton } from "./IconButton";
import { Portal } from "./Portal";

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** `confirm` is an `alertdialog`. */
  kind: "confirm" | "info";
  title: string;
  description: string;
  children?: ReactNode;
  /** Required in `confirm`. */
  confirm?: { label: string; variant: "danger" | "secondary"; onConfirm: () => void; pending?: boolean };
  /** Required in `info` ("Got it"). */
  acknowledge?: { label: string; onAcknowledge?: () => void };
  /** Default true: Escape and a scrim click close it. Cancel always does. */
  dismissible?: boolean;
  /** Default true in `info`, false in `confirm`. */
  showClose?: boolean;
  returnFocusRef?: RefObject<HTMLElement | null>;
}

export type DialogSurfaceProps = Omit<DialogProps, "open" | "onOpenChange" | "dismissible" | "returnFocusRef"> & {
  onCancel?: () => void;
  titleId?: string;
  descriptionId?: string;
};

/** The dialog as it looks: role, title, description and footer. With no `onCancel` it is a static picture (catalog). */
export function DialogSurface({ kind, title, description, children, confirm, acknowledge, showClose, onCancel, titleId, descriptionId }: DialogSurfaceProps) {
  const { t } = useTranslation();
  const ownTitle = useId();
  const ownDescription = useId();
  const tid = titleId ?? ownTitle;
  const did = descriptionId ?? ownDescription;
  const closeButton = showClose ?? kind === "info";

  return (
    <div
      role={kind === "confirm" ? "alertdialog" : "dialog"}
      aria-modal="true"
      aria-labelledby={tid}
      aria-describedby={did}
      className="relative flex w-(--gr-size-dialog-width) max-w-full flex-col gap-(--gr-space-4) rounded-lg bg-surface-raised p-(--gr-space-6) text-text-main shadow-(--gr-shadow-popover)"
    >
      <span aria-hidden="true" className="pointer-events-none absolute inset-0 rounded-lg shadow-(--gr-highlight)" />
      {closeButton ? (
        <IconButton
          variant="ghost"
          size="sm"
          label={t("ui.common.close")}
          icon={<Close size="sm" />}
          onClick={onCancel}
          className="absolute top-(--gr-space-3) right-(--gr-space-3)"
        />
      ) : null}
      <h2 id={tid} className="m-0 text-text-main [font:var(--gr-type-title)]">
        {title}
      </h2>
      <p id={did} className="m-0 text-text-muted [font:var(--gr-type-body)]">
        {description}
      </p>
      {children}
      <div className="flex justify-end gap-(--gr-space-2)">
        {kind === "confirm" && confirm ? (
          <>
            <Button data-autofocus onClick={onCancel}>
              {t("ui.common.cancel")}
            </Button>
            <Button variant={confirm.variant} onClick={confirm.onConfirm} {...(confirm.pending === undefined ? {} : { pending: confirm.pending })}>
              {confirm.label}
            </Button>
          </>
        ) : null}
        {kind === "info" && acknowledge ? (
          <Button data-autofocus onClick={acknowledge.onAcknowledge}>
            {acknowledge.label}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

/**
 * Siblings of `node` inside its container, and their direct children, become `inert`; returns what restores each
 * one's previous value. The children are redundant in a browser (inert is inherited) but not in jsdom, where the
 * app root sits one level below the sibling.
 */
function inertSiblings(node: Element): () => void {
  const restore: Array<() => void> = [];
  const mark = (el: Element) => {
    const had = el.hasAttribute("inert");
    el.setAttribute("inert", "");
    restore.push(() => {
      if (!had) el.removeAttribute("inert");
    });
  };
  for (const el of Array.from(node.parentElement?.children ?? [])) {
    if (el === node) continue;
    mark(el);
    Array.from(el.children).forEach(mark);
  }
  return () => restore.forEach((fn) => fn());
}

type LayerProps = Omit<DialogProps, "open" | "dismissible"> & { dismissible: boolean };

/** Mounted inside the Portal: scrim, trap, `inert` siblings and scroll lock live as long as it does. */
function DialogLayer({ onOpenChange, dismissible, returnFocusRef, ...surface }: LayerProps) {
  const content = useRef<HTMLDivElement>(null);
  const shown = useFadeIn();
  const close = () => onOpenChange(false);
  // Declared before the trap on purpose: cleanups run in declaration order, so `inert` is lifted first and the
  // trap can give the focus back. WebKit ignores focus() inside an inert subtree and the focus would fall to body.
  useEffect(() => {
    const node = content.current?.closest("[data-gr-portal]");
    if (!node) return;
    const undoInert = inertSiblings(node);
    const lockScroll = node.parentElement === document.body;
    const previous = document.body.style.overflow;
    if (lockScroll) document.body.style.overflow = "hidden";
    return () => {
      undoInert();
      if (lockScroll) document.body.style.overflow = previous;
    };
  }, []);

  useFocusTrap(content, true, returnFocusRef ? { returnFocus: returnFocusRef } : undefined);

  // On the document, not on the content: after a click on plain text the focus is on body and a handler there would miss Escape.
  useDismiss([], dismissible, close, { pointerOutside: false });

  const acknowledge = surface.acknowledge
    ? {
        label: surface.acknowledge.label,
        onAcknowledge: () => {
          surface.acknowledge?.onAcknowledge?.();
          close();
        },
      }
    : undefined;

  return (
    <>
      <div
        aria-hidden="true"
        onClick={dismissible ? close : undefined}
        className={cx("fixed inset-0 z-(--gr-z-scrim) bg-surface-scrim gr-motion", shown ? "opacity-100" : "opacity-0")}
      />
      <div className="pointer-events-none fixed inset-0 z-(--gr-z-dialog) flex items-center justify-center p-(--gr-window-margin)">
        <div ref={content} className={cx("pointer-events-auto max-w-full gr-motion", shown ? "opacity-100" : "opacity-0")}>
          <DialogSurface {...surface} {...(acknowledge ? { acknowledge } : {})} onCancel={close} />
        </div>
      </div>
    </>
  );
}

/** A modal (D14): focus starts on the safe button, Tab cycles inside, the rest of the app is inert, Enter is never intercepted. */
export function Dialog({ open, dismissible = true, ...props }: DialogProps) {
  if (import.meta.env.DEV) {
    if (props.kind === "confirm" && !props.confirm) throw new Error("Dialog kind=confirm needs the confirm prop");
    if (props.kind === "info" && !props.acknowledge) throw new Error("Dialog kind=info needs the acknowledge prop");
  }
  return open ? (
    <Portal>
      <DialogLayer {...props} dismissible={dismissible} />
    </Portal>
  ) : null;
}
