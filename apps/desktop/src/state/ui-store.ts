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

/** The open repo of the fleet; never persisted (DS-US-DSK-004 T003). */
export interface FleetSelection {
  selectedRepoId: string | null;
}

export interface UiActions {
  selectRepo: (id: string | null) => void;
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
      selectRepo: () => undefined,
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
