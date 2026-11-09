import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "auto" | "light" | "dark";
export type Language = "auto" | "en" | "es";

/** Cosmetic window preferences only: nothing here authorizes anything (ADR-GRP-017 § 1). */
export interface UiState {
  theme: Theme;
  language: Language;
  sidebarOpen: boolean;
  detailOpen: boolean;
}

/** What the center shows; never persisted (DS-US-DSK-008 D7). */
export type CenterView = { view: "main" } | { view: "pair"; repoId: string; pairId: string };

/** The open repo of the fleet; never persisted (DS-US-DSK-004 T003). */
export interface FleetSelection {
  selectedRepoId: string | null;
  /** The repo open before the current one: the fallback when the open one is closed. Not persisted. */
  previousRepoId: string | null;
  center: CenterView;
}

export interface UiActions {
  selectRepo: (id: string | null) => void;
  openPairDetail: (target: { repoId: string; pairId: string }) => void;
  backToMain: () => void;
  closePairDetail: () => void;
  toggleSidebar: () => void;
  toggleDetail: () => void;
  cycleTheme: () => void;
  setLanguage: (language: Language) => void;
}

const NEXT_THEME: Record<Theme, Theme> = { auto: "light", light: "dark", dark: "auto" };

/** The window store, persisted in `localStorage` under `gitraptor.ui.v1` (version 1). */
export const useUiStore = create<UiState & UiActions & FleetSelection>()(
  persist(
    (set) => ({
      theme: "auto",
      language: "auto",
      sidebarOpen: true,
      detailOpen: true,
      selectedRepoId: null,
      previousRepoId: null,
      center: { view: "main" },
      openPairDetail: ({ repoId, pairId }) => set({ center: { view: "pair", repoId, pairId } }),
      backToMain: () => set({ center: { view: "main" } }),
      closePairDetail: () => set({ center: { view: "main" } }),
      selectRepo: (id) =>
        set((s) =>
          id === null || s.selectedRepoId === null || s.selectedRepoId === id
            ? { selectedRepoId: id, center: s.selectedRepoId === id ? s.center : { view: "main" } }
            : { selectedRepoId: id, previousRepoId: s.selectedRepoId, center: { view: "main" } },
        ),
      toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
      toggleDetail: () => set((s) => ({ detailOpen: !s.detailOpen })),
      cycleTheme: () => set((s) => ({ theme: NEXT_THEME[s.theme] })),
      setLanguage: (language) => set({ language }),
    }),
    {
      name: "gitraptor.ui.v1",
      version: 1,
      partialize: ({ theme, language, sidebarOpen, detailOpen }): UiState => ({ theme, language, sidebarOpen, detailOpen }),
    },
  ),
);
