import { createContext, useContext, useEffect, useMemo, useReducer, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";
import { cx, type CssVars } from "./cx";
import { useFadeIn } from "./hooks/useFadeIn";
import { Check, CircleAlert, Close, OctagonX } from "./icons";
import { IconButton } from "./IconButton";
import { Portal } from "./Portal";

export type ToastKind = "success" | "info" | "warning" | "error";

export interface ToastInput {
  kind: ToastKind;
  message: string;
  /** Replaces the hidden kind prefix (`ui.toast.kind.<kind>`) read by screen readers, e.g. "Blocked" (DS-US-DSK-007 D15). */
  srLabel?: string;
  action?: { label: string; onAction: () => void };
}

type ToastItem = ToastInput & { id: string };
type Action = { type: "add"; toast: ToastItem } | { type: "remove"; id: string };

const AUTO_CLOSE_MS = 5000;
const MAX_VISIBLE = 3;

const GLYPH = {
  success: { Icon: Check, tone: "text-success-text" },
  warning: { Icon: CircleAlert, tone: "text-warning-text" },
  error: { Icon: OctagonX, tone: "text-danger-text" },
} as const;

function reducer(queue: ToastItem[], action: Action): ToastItem[] {
  return action.type === "add" ? [...queue, action.toast] : queue.filter((t) => t.id !== action.id);
}

interface ToastApi {
  show: (toast: ToastInput) => string;
  dismiss: (id: string) => void;
}
const ToastContext = createContext<ToastApi | null>(null);

/** Shows and dismisses toasts. Needs a `ToastProvider` above it. */
export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) throw new Error("useToast needs a ToastProvider");
  return api;
}

/** Like `useToast`, but `null` outside a `ToastProvider`: for views that raise a toast only when the app has one. */
export function useOptionalToast(): ToastApi | null {
  return useContext(ToastContext);
}

/** The toast as it looks, with no timer (the catalog paints it). `role` follows the kind; Escape inside closes it. */
export function ToastSurface({ kind, message, srLabel, action, onClose, id }: ToastInput & { onClose: () => void; id?: string }) {
  const { t } = useTranslation();
  const glyph = kind === "info" ? null : GLYPH[kind];
  return (
    <div
      id={id}
      data-toast=""
      role={kind === "error" ? "alert" : "status"}
      tabIndex={-1}
      onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onClose();
        }
      }}
      className="flex max-w-(--gr-size-detail) items-center gap-(--gr-space-2) rounded-md bg-surface-raised px-(--gr-space-4) py-(--gr-space-3) text-text-main shadow-(--gr-shadow-popover) gr-ring [font:var(--gr-type-body)]"
    >
      {glyph ? (
        <span className={cx("inline-flex shrink-0", glyph.tone)}>
          <glyph.Icon />
        </span>
      ) : null}
      {kind !== "info" ? <span className="sr-only">{srLabel ?? t(`ui.toast.kind.${kind}`)}</span> : null}
      <span className="min-w-0 flex-1">{message}</span>
      {action ? (
        <Button
          size="sm"
          onClick={() => {
            action.onAction();
            onClose();
          }}
        >
          {action.label}
        </Button>
      ) : null}
      <IconButton variant="ghost" size="sm" label={t("ui.common.close")} icon={<Close size="sm" />} onClick={onClose} />
    </div>
  );
}

/** One visible toast: its auto-close timer runs while not paused and resumes with the time it had left. */
function ToastCard({ toast, paused, onClose }: { toast: ToastItem; paused: boolean; onClose: () => void }) {
  const shown = useFadeIn();
  const remaining = useRef(AUTO_CLOSE_MS);
  const latest = useRef(onClose);
  latest.current = onClose;

  useEffect(() => {
    if (toast.kind === "error" || paused) return;
    const startedAt = Date.now();
    const timer = setTimeout(() => latest.current(), remaining.current);
    return () => {
      clearTimeout(timer);
      remaining.current -= Date.now() - startedAt;
    };
  }, [paused, toast.kind]);

  return (
    <div className={cx("gr-motion", shown ? "opacity-100" : "opacity-0")}>
      <ToastSurface
        kind={toast.kind}
        message={toast.message}
        onClose={onClose}
        {...(toast.action ? { action: toast.action } : {})}
        {...(toast.srLabel ? { srLabel: toast.srLabel } : {})}
      />
    </div>
  );
}

function Viewport({ toasts, dismiss }: { toasts: ToastItem[]; dismiss: (id: string) => void }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLElement>(null);
  const [hovering, setHovering] = useState(false);
  const [focusWithin, setFocusWithin] = useState(false);
  const [windowBlurred, setWindowBlurred] = useState(false);
  const [top, setTop] = useState(false);

  useEffect(() => {
    const blur = () => setWindowBlurred(true);
    const focus = () => setWindowBlurred(false);
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === "F8") ref.current?.querySelector<HTMLElement>("[data-toast]")?.focus();
    };
    // The viewport moves to the top while the focused element sits under it (design-flow § 7).
    const onFocusIn = (e: FocusEvent) => {
      const viewport = ref.current;
      if (!viewport || !(e.target instanceof Element) || viewport.contains(e.target)) return setTop(false);
      const a = e.target.getBoundingClientRect();
      const b = viewport.getBoundingClientRect();
      setTop(a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top);
    };
    const onFocusOut = () => setTop(false);
    window.addEventListener("blur", blur);
    window.addEventListener("focus", focus);
    window.addEventListener("keydown", onKeyDown);
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("focusout", onFocusOut);
    return () => {
      window.removeEventListener("blur", blur);
      window.removeEventListener("focus", focus);
      window.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("focusout", onFocusOut);
    };
  }, []);

  const paused = hovering || focusWithin || windowBlurred;
  const style: CssVars | undefined = top ? { top: "calc(var(--gr-size-titlebar) + var(--gr-size-toolbar))" } : undefined;

  return (
    <section
      ref={ref}
      aria-label={t("ui.toast.region")}
      data-position={top ? "top" : "bottom"}
      style={style}
      onPointerEnter={() => setHovering(true)}
      onPointerLeave={() => setHovering(false)}
      onFocus={() => setFocusWithin(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setFocusWithin(false);
      }}
      className={cx(
        "pointer-events-none fixed inset-x-0 z-(--gr-z-toast) mx-auto flex w-fit flex-col gap-(--gr-space-2)",
        !top && "bottom-(--gr-window-margin)",
      )}
    >
      {toasts.slice(0, MAX_VISIBLE).map((toast) => (
        <div key={toast.id} className="pointer-events-auto">
          <ToastCard toast={toast} paused={paused} onClose={() => dismiss(toast.id)} />
        </div>
      ))}
    </section>
  );
}

/** Holds the queue (3 visible, the rest wait) and renders the viewport in a portal. One per app, one per catalog panel. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [queue, dispatch] = useReducer(reducer, []);
  const counter = useRef(0);
  const api = useMemo<ToastApi>(
    () => ({
      show: (toast) => {
        counter.current += 1;
        const id = `toast-${counter.current}`;
        dispatch({ type: "add", toast: { ...toast, id } });
        return id;
      },
      dismiss: (id) => dispatch({ type: "remove", id }),
    }),
    [],
  );

  return (
    <ToastContext value={api}>
      {children}
      <Portal>
        <Viewport toasts={queue} dismiss={api.dismiss} />
      </Portal>
    </ToastContext>
  );
}
