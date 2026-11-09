/** Focus bookkeeping of the conflict views: the panel heading and the "View" button that opened a pair (D7, D16, D18). */
export const PANEL_TITLE_ID = "gr-conflict-panel-title";

const openers = new Map<string, HTMLButtonElement>();
const key = (repoId: string, pairId: string) => `${repoId}\u0000${pairId}`;

/** Callback ref for the "View" button of a band; the returned cleanup forgets it. */
export function registerOpener(repoId: string, pairId: string) {
  return (node: HTMLButtonElement | null) => {
    if (node === null) return;
    openers.set(key(repoId, pairId), node);
    return () => {
      if (openers.get(key(repoId, pairId)) === node) openers.delete(key(repoId, pairId));
    };
  };
}

/** The heading of the conflict panel takes the focus (it has `tabIndex={-1}`). */
export function focusConflictPanel(): void {
  document.getElementById(PANEL_TITLE_ID)?.focus();
}

/** Back from a pair detail: the "View" button that opened it, else the panel heading. */
export function focusOpener(repoId: string, pairId: string): void {
  const button = openers.get(key(repoId, pairId));
  if (button?.isConnected) button.focus();
  else focusConflictPanel();
}
