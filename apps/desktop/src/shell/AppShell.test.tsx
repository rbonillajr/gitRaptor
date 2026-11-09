// B1 (TS-DSK-001 T007): the empty B1 window. Regions with accessible names in en and es, shortcuts, theme
// cycle, a focus-trapping drawer at 1200 px, axe without violations and the gr:shell-painted mark.
//
// Assumptions the expert must provide (the plan names none of them):
//  - `tests/setup.ts` exports `setViewportWidth(px: number)`: it makes the simulated `matchMedia`
//    answer min-width / max-width queries for that width (default 1440).
//  - `AppShell` is a named export of ./AppShell; it needs a QueryClientProvider (the test wraps it).
//  - The title bar has exactly three buttons, in order: left panel, theme, right panel.
//  - The drawer traps Tab itself: Tab on its last focusable (Shift+Tab on its first) is default-prevented.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { EngineMessage } from "@/client/engine-types";
import { tienda } from "@/fleet/fixtures";
import { applyEngineBatch } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { setViewportWidth } from "../../tests/setup";
import { AppShell } from "./AppShell";

const mac = () => vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X)");

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <AppShell />
    </QueryClientProvider>,
  );
}

const titleButtons = () => within(screen.getByRole("banner")).getAllByRole("button");
const isOn = (b: HTMLElement) => (b.getAttribute("aria-pressed") ?? b.getAttribute("aria-expanded")) === "true";
const key = (init: KeyboardEventInit) => fireEvent.keyDown(window, init);

beforeEach(async () => {
  mockIPC(() => ({ version: "0.0.0", protocol: 1 }));
  useUiStore.setState({ theme: "auto", language: "auto", sidebarOpen: true, detailOpen: true });
  document.documentElement.removeAttribute("data-theme");
  setViewportWidth(1440);
  mac();
  await act(async () => {
    await i18n.changeLanguage("en");
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("B1 empty shell", () => {
  it("B1: shows title bar, sidebar, center and detail with accessible names in en and es", async () => {
    const names = async () => {
      const view = mount();
      const out = {
        banner: screen.getByRole("banner"),
        nav: screen.getByRole("navigation"),
        main: screen.getByRole("main"),
        aside: screen.getByRole("complementary"),
      };
      const result = [out.nav, out.aside].map((el) => el.getAttribute("aria-label") ?? "");
      result.push(...titleButtons().map((b) => b.getAttribute("aria-label") ?? ""));
      expect(titleButtons(), "title bar must have exactly three buttons").toHaveLength(3);
      for (const n of result) expect(n.trim().length, "every region and button needs an accessible name").toBeGreaterThan(0);
      view.unmount();
      return result;
    };
    const en = await names();
    await act(async () => {
      await i18n.changeLanguage("es");
    });
    const es = await names();
    expect(es, "Spanish names must differ from English ones").not.toEqual(en);
    for (let i = 0; i < en.length; i += 1) expect(es[i], `name ${i} untranslated`).not.toBe(en[i]);
  });

  it("B1: Cmd+B toggles the sidebar and Alt+Cmd+B the detail", () => {
    mount();
    const [left, , right] = titleButtons() as [HTMLElement, HTMLElement, HTMLElement];
    expect([isOn(left), isOn(right)]).toEqual([true, true]);
    key({ code: "KeyB", metaKey: true });
    expect(useUiStore.getState().sidebarOpen, "Cmd+B must fold the sidebar").toBe(false);
    expect(isOn(left)).toBe(false);
    expect(useUiStore.getState().detailOpen, "Cmd+B must not touch the detail").toBe(true);
    key({ code: "KeyB", metaKey: true });
    expect(useUiStore.getState().sidebarOpen).toBe(true);
    key({ code: "KeyB", metaKey: true, altKey: true, key: "∫" });
    expect(useUiStore.getState().detailOpen, "Alt+Cmd+B must fold the detail (compared by event.code)").toBe(false);
    expect(isOn(right)).toBe(false);
    key({ code: "KeyB", metaKey: true, altKey: true, key: "∫" });
    expect(useUiStore.getState().detailOpen).toBe(true);
  });

  it("B1: the theme button cycles auto -> light -> dark -> auto and sets data-theme", () => {
    mount();
    const root = document.documentElement;
    const theme = titleButtons()[1] as HTMLElement;
    expect(root.hasAttribute("data-theme"), "auto removes data-theme").toBe(false);
    const seen: (string | null)[] = [];
    for (let i = 0; i < 3; i += 1) {
      fireEvent.click(theme);
      seen.push(root.getAttribute("data-theme"));
    }
    expect(seen).toEqual(["light", "dark", null]);
    fireEvent.click(theme);
    const saved = JSON.parse(localStorage.getItem("gitraptor.ui.v1") ?? "null") as { state: { theme: string } } | null;
    expect(saved?.state.theme, "theme must persist").toBe("light");
  });

  it("B1: at 1200 px the detail is a closed drawer that Alt+Cmd+B opens with focus inside and Esc closes", () => {
    setViewportWidth(1200);
    mount();
    const right = titleButtons()[2] as HTMLElement;
    expect(isOn(right), "the drawer starts closed").toBe(false);
    right.focus();
    key({ code: "KeyB", metaKey: true, altKey: true });
    expect(isOn(right), "Alt+Cmd+B opens the drawer").toBe(true);
    const aside = screen.getByRole("complementary");
    expect(aside.contains(document.activeElement), "focus must move inside the drawer").toBe(true);

    const focusable = Array.from(aside.querySelectorAll<HTMLElement>("button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])"));
    const first = focusable[0] ?? aside;
    const last = focusable[focusable.length - 1] ?? aside;
    last.focus();
    expect(fireEvent.keyDown(last, { key: "Tab" }), "Tab on the last element must be wrapped (default prevented)").toBe(false);
    expect(aside.contains(document.activeElement), "Tab must not leave the drawer").toBe(true);
    first.focus();
    expect(fireEvent.keyDown(first, { key: "Tab", shiftKey: true }), "Shift+Tab on the first element must be wrapped").toBe(false);
    expect(aside.contains(document.activeElement), "Shift+Tab must not leave the drawer").toBe(true);

    key({ key: "Escape", code: "Escape" });
    expect(isOn(right), "Esc closes the drawer").toBe(false);
    expect(document.activeElement, "focus returns to the button").toBe(right);
  });

  it.each([1440, 1200])("B1: axe finds no violations at %i px", async (width) => {
    setViewportWidth(width);
    const { container } = mount();
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });

  it("B1: marks gr:shell-painted", async () => {
    performance.clearMarks();
    mount();
    await waitFor(() => expect(performance.getEntriesByName("gr:shell-painted").length, "gr:shell-painted mark").toBeGreaterThan(0));
  });
});

// D3-CENTER (DS-US-DSK-003 T007): the window mounts EngineBand and EngineCenter, fed by `useEngine` from the engine
// store and the query cache. The engine modules are imported inside the tests so the B1 tests above stay independent.
describe("D3-CENTER engine states in the window", () => {
  const SESSION = { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true };
  const global = (state: "waiting-for-git" | "no-repos" | "observing") => ({
    engine: { state, gitVersion: state === "waiting-for-git" ? null : "2.45.1" },
    daemon: { version: "0.1.0", protocol: 9 },
    autostart: "unknown",
    repos: [],
  });
  let invoked: { cmd: string; args: unknown }[] = [];

  async function mountEngine(conn: Record<string, unknown>, data: ReturnType<typeof global> | null, staleSinceMs: number | null = null) {
    invoked = [];
    mockIPC((cmd, args) => {
      invoked.push({ cmd, args });
      return cmd === "app_info" ? { version: "0.0.0", protocol: 1 } : {};
    });
    const { engineKeys } = await import("@/client/queries");
    const { useEngineStore } = await import("@/state/engine-store");
    useEngineStore.setState({ conn, staleSinceMs, resync: null, session: SESSION } as never);
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
    if (data) client.setQueryData(engineKeys.global, data);
    return render(
      <QueryClientProvider client={client}>
        <AppShell />
      </QueryClientProvider>,
    );
  }

  it("D3-CENTER: the centre shows the panel engineView gives (waiting for Git)", async () => {
    await mountEngine({ kind: "live" }, global("waiting-for-git"));
    const main = screen.getByRole("main");
    expect(within(main).getByRole("heading", { name: "Waiting for Git" })).toBeInTheDocument();
    expect(main.querySelector("[data-center]")).toHaveAttribute("data-center", "waiting-for-git");
    expect(main.querySelector("[data-center]")).not.toHaveAttribute("data-stale");
  });

  it("D3-CENTER: an incompatible engine replaces the centre with its panel", async () => {
    await mountEngine(
      { kind: "incompatible", which: "app-older", daemonVersion: "9.9.0", daemonPath: null, stopCommand: null, action: "update-app" },
      global("observing"),
    );
    const main = screen.getByRole("main");
    expect(main.querySelector("[data-center]")).toHaveAttribute("data-center", "error");
    expect(within(main).getByRole("heading", { name: "Incompatible engine" })).toBeInTheDocument();
  });

  it("D3-CENTER: the centre is data-stale and the band explains when the connection is not live", async () => {
    const since = new Date(2026, 10, 9, 14, 5).getTime();
    await mountEngine({ kind: "reconnecting", attempt: 1, retryInMs: 2000 }, global("observing"), since);
    const main = screen.getByRole("main");
    expect(main.querySelector("[data-center]"), "rows 5 to 7 are dimmed while not live").toHaveAttribute("data-stale", "true");
    expect(screen.getByText(/Lost the connection to the engine\./)).toBeInTheDocument();
  });

  it("D3-CENTER: the Start engine button calls engine_retry with start true", async () => {
    await mountEngine({ kind: "stopped", reason: "not-running" }, null);
    expect(screen.getByRole("main").querySelector("[data-center]")).toHaveAttribute("data-center", "stopped");
    const [start] = screen.getAllByRole("button", { name: "Start engine" });
    if (!start) throw new Error("no Start engine button");
    fireEvent.click(start);
    expect(invoked.filter((i) => i.cmd === "engine_retry")).toEqual([{ cmd: "engine_retry", args: { params: { start: true } } }]);
  });
});

// D4-SHELL (DS-US-DSK-004 T005): the sidebar holds the RepoTree, the centre holds the FleetView of the open repo
// (skeletons with aria-busy until the repo snapshot is in), and choosing a repo in the tree opens it.
describe("D4 fleet shell", () => {
  const SESSION = { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true };
  let invoked: { cmd: string; args: unknown }[] = [];
  const repoRow = (repoId: string, name: string) => ({
    repoId,
    name,
    path: `/w/${name}/.git`,
    state: "observed" as const,
    tier: "active" as const,
    attention: { conflicts: { state: "counted" as const, count: 0 }, denials: { state: "counted" as const, count: 0 } },
  });

  async function mountFleet(messages: (client: QueryClient, fleet: ReturnType<typeof tienda>) => void) {
    invoked = [];
    mockIPC((cmd, args) => {
      invoked.push({ cmd, args });
      return cmd === "app_info" ? { version: "0.0.0", protocol: 1 } : {};
    });
    const { useEngineStore } = await import("@/state/engine-store");
    useEngineStore.setState({ conn: { kind: "live" }, staleSinceMs: null, resync: null, session: SESSION });
    useUiStore.setState({ selectedRepoId: null });
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
    messages(client, tienda());
    return render(
      <QueryClientProvider client={client}>
        <AppShell />
      </QueryClientProvider>,
    );
  }
  const globalWith = (...names: string[]): EngineMessage => ({
    type: "global",
    engine: { state: "observing", gitVersion: "2.45.1" },
    daemon: { version: "0.1.0", protocol: 9 },
    autostart: "unknown",
    repos: names.map((n) => repoRow(n, n)),
  });

  it("fleet shell: the sidebar holds the repo tree with a row per repo", async () => {
    await mountFleet((client) => applyEngineBatch(client, [globalWith("notas", "tienda")]));
    const nav = screen.getByRole("navigation");
    const tree = within(nav).getByRole("tree");
    expect(within(tree).getAllByRole("treeitem", { name: /^(notas|tienda)/ }).length, "one row per repo").toBeGreaterThanOrEqual(2);
    expect(within(nav).getByText("Repos"), "the tree header").toBeInTheDocument();
  });

  it("fleet shell: the centre paints the fleet of the open repo and the tree lists its worktrees", async () => {
    await mountFleet((client, fleet) => applyEngineBatch(client, [globalWith("tienda", "zeta"), { type: "repo", fleet: fleet.repo }]));
    const main = screen.getByRole("main");
    await within(main).findByRole("list", { name: "Worktrees of tienda" });
    expect(within(main).getAllByRole("article"), "one card per worktree").toHaveLength(3);
    expect(useUiStore.getState().selectedRepoId, "the first repo by name is open by default").toBe("tienda");
  });

  it("fleet shell: the open repo defaults to the first by name and fleet_open is called for it", async () => {
    await mountFleet((client) => applyEngineBatch(client, [globalWith("tienda", "notas")]));
    await waitFor(() => expect(invoked.filter((i) => i.cmd === "fleet_open")).toEqual([{ cmd: "fleet_open", args: { params: { repoId: "notas" } } }]));
  });

  it("fleet shell: without the repo snapshot the centre is skeletons with aria-busy, never no agents", async () => {
    await mountFleet((client) => applyEngineBatch(client, [globalWith("tienda")]));
    const main = screen.getByRole("main");
    await waitFor(() => expect(main.querySelector('[aria-busy="true"]'), "a busy region while the snapshot is out").not.toBeNull());
    expect(within(main).queryByText(/No agents are working/), "never says no agents without the snapshot").toBeNull();
    expect(within(main).queryAllByRole("article"), "no card yet").toHaveLength(0);
  });

  it("fleet shell: choosing a repo in the tree opens it", async () => {
    await mountFleet((client, fleet) =>
      applyEngineBatch(client, [globalWith("notas", "tienda"), { type: "repo", fleet: { ...fleet.repo, repoId: "notas", name: "notas" } }]),
    );
    const nav = screen.getByRole("navigation");
    const row = within(nav)
      .getAllByRole("treeitem")
      .find((r) => r.getAttribute("aria-level") === "1" && /^tienda/.test(r.getAttribute("aria-label") ?? r.textContent ?? ""));
    if (!row) throw new Error("no tienda row in the tree");
    fireEvent.click(row);
    await waitFor(() => expect(useUiStore.getState().selectedRepoId).toBe("tienda"));
    await waitFor(() => expect(invoked.filter((i) => i.cmd === "fleet_open").map((i) => (i.args as { params: { repoId: string } }).params.repoId)).toContain("tienda"));
  });
});
