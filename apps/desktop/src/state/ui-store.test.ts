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

describe("D8 ui store center (DS-US-DSK-008 D7)", () => {
  beforeEach(() => {
    useUiStore.setState({ selectedRepoId: null, center: { view: "main" } });
  });

  it("the center starts on main", () => {
    expect(useUiStore.getState().center, "the center has one destination at a time, main by default").toEqual({ view: "main" });
  });

  it("opening a pair replaces the center view and back returns to main", () => {
    useUiStore.getState().openPairDetail({ repoId: "tienda", pairId: "p1" });
    expect(useUiStore.getState().center, "openPairDetail sets the pair destination").toEqual({ view: "pair", repoId: "tienda", pairId: "p1" });
    useUiStore.getState().openPairDetail({ repoId: "tienda", pairId: "p2" });
    expect(useUiStore.getState().center, "opening another pair replaces the first").toEqual({ view: "pair", repoId: "tienda", pairId: "p2" });
    useUiStore.getState().backToMain();
    expect(useUiStore.getState().center, "backToMain returns to the main view, not to the previous pair").toEqual({ view: "main" });
    useUiStore.getState().openPairDetail({ repoId: "tienda", pairId: "p1" });
    useUiStore.getState().closePairDetail();
    expect(useUiStore.getState().center, "closePairDetail is an alias of backToMain").toEqual({ view: "main" });
  });

  it("selecting another repo returns to main", () => {
    useUiStore.getState().selectRepo("tienda");
    useUiStore.getState().openPairDetail({ repoId: "tienda", pairId: "p1" });
    expect(useUiStore.getState().center.view, "the pair is open before changing repo").toBe("pair");
    useUiStore.getState().selectRepo("notas");
    expect(useUiStore.getState().center, "changing repo leaves the pair view").toEqual({ view: "main" });
    expect(useUiStore.getState().selectedRepoId).toBe("notas");
  });

  it("the center is not persisted", () => {
    useUiStore.getState().openPairDetail({ repoId: "tienda", pairId: "p1" });
    expect(useUiStore.getState().center.view, "the pair is open in memory").toBe("pair");
    useUiStore.getState().cycleTheme();
    expect(Object.keys(stored()?.state ?? {}).sort(), "the four keys stay the only ones").toEqual(["detailOpen", "language", "sidebarOpen", "theme"]);
    expect(localStorage.getItem(KEY) ?? "", "the pair id never reaches localStorage").not.toContain("p1");
  });
});

describe("D7 ui store guardrails (DS-US-DSK-007 T003)", () => {
  beforeEach(() => {
    useUiStore.setState({ selectedRepoId: null, center: { view: "main" } });
  });

  it("openGuardrails puts the guardrails view in the center and backToMain returns", () => {
    useUiStore.getState().openGuardrails();
    expect(useUiStore.getState().center, "openGuardrails sets the guardrails destination").toEqual({ view: "guardrails" });
    useUiStore.getState().backToMain();
    expect(useUiStore.getState().center).toEqual({ view: "main" });
  });

  it("openGuardrails replaces an open pair", () => {
    useUiStore.getState().openPairDetail({ repoId: "tienda", pairId: "p1" });
    useUiStore.getState().openGuardrails();
    expect(useUiStore.getState().center, "the guardrails view replaces the pair").toEqual({ view: "guardrails" });
  });

  it("openGuardrails is not persisted and the four keys stay the only ones", () => {
    useUiStore.getState().openGuardrails();
    useUiStore.getState().cycleTheme();
    expect(useUiStore.getState().center.view, "the view is open in memory").toBe("guardrails");
    expect(Object.keys(stored()?.state ?? {}).sort(), "the four keys stay the only ones").toEqual(["detailOpen", "language", "sidebarOpen", "theme"]);
    expect(localStorage.getItem(KEY) ?? "", "the view never reaches localStorage").not.toContain("guardrails");
  });
});
