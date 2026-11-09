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

describe("D4 ui store selectedRepoId", () => {
  beforeEach(() => {
    useUiStore.setState({ selectedRepoId: null });
  });

  it("selectedRepoId: starts null and selectRepo sets and clears it", () => {
    expect(useUiStore.getState().selectedRepoId, "the chosen repo starts as null").toBeNull();
    expect(typeof useUiStore.getState().selectRepo, "the store has selectRepo").toBe("function");
    useUiStore.getState().selectRepo("tienda");
    expect(useUiStore.getState().selectedRepoId).toBe("tienda");
    useUiStore.getState().selectRepo(null);
    expect(useUiStore.getState().selectedRepoId).toBeNull();
  });

  it("selectedRepoId: is not persisted, the four keys stay the only ones", () => {
    expect(typeof useUiStore.getState().selectRepo, "the store has selectRepo").toBe("function");
    useUiStore.getState().selectRepo("tienda-id-from-the-engine");
    useUiStore.getState().cycleTheme();
    const saved = stored();
    expect(saved, "something is persisted").not.toBeNull();
    expect(Object.keys(saved?.state ?? {}).sort(), "no fifth key").toEqual(["detailOpen", "language", "sidebarOpen", "theme"]);
    expect(localStorage.getItem(KEY) ?? "", "the engine id never reaches localStorage").not.toContain("tienda-id-from-the-engine");
  });

  it("selectedRepoId: lives in memory, a rehydrate does not clear it", async () => {
    expect(typeof useUiStore.getState().selectRepo, "the store has selectRepo").toBe("function");
    useUiStore.getState().selectRepo("tienda");
    localStorage.setItem(KEY, JSON.stringify({ state: { theme: "dark", language: "es", sidebarOpen: false, detailOpen: false }, version: 1 }));
    await useUiStore.persist.rehydrate();
    expect(useUiStore.getState().selectedRepoId, "the choice is not stored, so rehydrating leaves it alone").toBe("tienda");
  });
});
