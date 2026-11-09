// FE2 (TS-DSK-001 T006): the window store cycles the theme auto -> light -> dark -> auto, persists only
// theme, language, sidebarOpen and detailOpen under gitraptor.ui.v1 (version 1) and rehydrates.
// Assumed name (the plan does not fix it): `useUiStore`, a Zustand store with `persist`.
import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./ui-store";

const KEY = "gitraptor.ui.v1";
const DEFAULTS = { theme: "auto", language: "auto", sidebarOpen: true, detailOpen: true } as const;

beforeEach(() => {
  useUiStore.setState({ ...DEFAULTS });
  localStorage.clear();
});

const stored = () =>
  JSON.parse(localStorage.getItem(KEY) ?? "null") as { state: Record<string, unknown>; version: number } | null;

describe("FE2 ui store", () => {
  it("FE2: defaults are auto, auto, open, open", () => {
    const s = useUiStore.getState();
    expect({ theme: s.theme, language: s.language, sidebarOpen: s.sidebarOpen, detailOpen: s.detailOpen }).toEqual(DEFAULTS);
  });

  it("FE2: cycleTheme goes auto -> light -> dark -> auto", () => {
    const seen: string[] = [];
    for (let i = 0; i < 3; i += 1) {
      useUiStore.getState().cycleTheme();
      seen.push(useUiStore.getState().theme);
    }
    expect(seen).toEqual(["light", "dark", "auto"]);
  });

  it("FE2: toggles and setLanguage update the state", () => {
    useUiStore.getState().toggleSidebar();
    useUiStore.getState().toggleDetail();
    useUiStore.getState().setLanguage("es");
    const s = useUiStore.getState();
    expect([s.sidebarOpen, s.detailOpen, s.language]).toEqual([false, false, "es"]);
  });

  it("FE2: persists exactly theme, language, sidebarOpen and detailOpen, version 1", () => {
    useUiStore.getState().cycleTheme();
    useUiStore.getState().toggleSidebar();
    const saved = stored();
    expect(saved, "nothing persisted under gitraptor.ui.v1").not.toBeNull();
    expect(saved?.version).toBe(1);
    expect(Object.keys(saved?.state ?? {}).sort()).toEqual(["detailOpen", "language", "sidebarOpen", "theme"]);
    expect(saved?.state).toEqual({ theme: "light", language: "auto", sidebarOpen: false, detailOpen: true });
  });

  it("FE2: rehydrates from localStorage", async () => {
    const state = { theme: "dark", language: "es", sidebarOpen: false, detailOpen: false };
    localStorage.setItem(KEY, JSON.stringify({ state, version: 1 }));
    await useUiStore.persist.rehydrate();
    const s = useUiStore.getState();
    expect({ theme: s.theme, language: s.language, sidebarOpen: s.sidebarOpen, detailOpen: s.detailOpen }).toEqual(state);
  });
});
