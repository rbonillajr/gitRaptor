import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { GuardBlocks } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { useOptionalToast } from "@/ui";
import { blockRows, unseen } from "./blocks-model";
import { useGuardUnseen } from "./unseen-store";

/**
 * The notice of a new block (D11, D15): counts the denials newer than the last time the person looked (what the first
 * read of a repo found is history, not news), and keeps at most one persistent error toast for them. Opening the
 * Guardrails view marks them seen and retires the toast, as does opening the dialog. Mount it once, in `EngineCenter`.
 */
export function useGuardAlerts(data: GuardBlocks | undefined, dialogOpen: boolean, onOpenRow: (id: string) => void): void {
  const { t, i18n } = useTranslation();
  const toast = useOptionalToast();
  const selected = useUiStore((s) => s.selectedRepoId);
  const inView = useUiStore((s) => s.center.view === "guardrails");
  const seenAfter = useRef(new Map<string, number>());
  const shown = useRef<{ toastId: string | null; key: string | null }>({ toastId: null, key: null });
  const latest = useRef({ onOpenRow, toast });
  useEffect(() => {
    latest.current = { onOpenRow, toast };
  });

  useEffect(() => {
    const retire = () => {
      if (shown.current.toastId !== null) latest.current.toast?.dismiss(shown.current.toastId);
      shown.current.toastId = null;
    };
    const setCount = useGuardUnseen.getState().set;
    if (selected === null || data === undefined) {
      setCount(0);
      retire();
      return;
    }
    const newest = data.entries.reduce((max, e) => Math.max(max, e.lastMs), 0);
    if (!seenAfter.current.has(selected) && data.state === "ready") seenAfter.current.set(selected, newest);
    const after = seenAfter.current.get(selected);
    if (after === undefined) return;
    if (inView) {
      seenAfter.current.set(selected, Math.max(after, newest));
      setCount(0);
      retire();
      shown.current.key = null;
      return;
    }
    const fresh = unseen(data.entries, after);
    setCount(fresh.length);
    if (fresh.length === 0) {
      retire();
      shown.current.key = null;
      return;
    }
    if (dialogOpen) {
      retire();
      return;
    }
    const key = fresh
      .map((e) => e.decisionId)
      .sort()
      .join("|");
    if (shown.current.key === key || latest.current.toast === null) return;
    retire();
    shown.current.key = key;
    const only = fresh.length === 1 ? fresh[0] : undefined;
    const title = only === undefined ? undefined : blockRows(data, true, t, i18n.language).find((r) => r.id === only.decisionId)?.title;
    shown.current.toastId = latest.current.toast.show({
      kind: "error",
      srLabel: t("grd.toast.kindLabel"),
      message: only === undefined || title === undefined ? t("grd.toast.many", { count: fresh.length }) : title,
      action: {
        label: t("grd.toast.view"),
        onAction: () => {
          if (only !== undefined) latest.current.onOpenRow(only.decisionId);
          else useUiStore.getState().openGuardrails();
        },
      },
    });
  }, [data, selected, inView, dialogOpen, t, i18n.language]);
}
