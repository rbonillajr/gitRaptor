// D3-SHELL (DS-US-DSK-003 T007): the status pill sits in the title bar between the left panel button and the theme
// button. It is checked through the window (`AppShell`), which wires the title bar to the engine store and the cache;
// the plan does not say whether `TitleBar` takes the pill as a prop or reads it, so the test does not guess.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { PairDto } from "@/client/engine-types";
import { NOW, pair, tienda } from "@/fleet/fixtures";
import { applyEngineBatch } from "@/state/engine-cache";
import { useUiStore } from "@/state/ui-store";
import { setViewportWidth } from "../../tests/setup";
import { AppShell } from "./AppShell";

const GLOBAL = {
  engine: { state: "observing", gitVersion: "2.45.1" },
  daemon: { version: "0.1.0", protocol: 9 },
  autostart: "unknown",
  repos: [],
};

async function mountLive() {
  const { engineKeys } = await import("@/client/queries");
  const { useEngineStore } = await import("@/state/engine-store");
  useEngineStore.setState({
    conn: { kind: "live" },
    staleSinceMs: null,
    resync: null,
    session: { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true },
  });
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  client.setQueryData(engineKeys.global, GLOBAL);
  render(
    <QueryClientProvider client={client}>
      <AppShell />
    </QueryClientProvider>,
  );
}

beforeEach(async () => {
  mockIPC(() => ({ version: "0.0.0", protocol: 1 }));
  await act(async () => {
    await i18n.changeLanguage("en");
  });
});

afterEach(() => {
  cleanup();
  setViewportWidth(1440);
  clearMocks();
});

describe("D3-SHELL title bar pill", () => {
  it("D3-SHELL: the pill is in the title bar between the left panel button and the theme button", async () => {
    await mountLive();
    const banner = screen.getByRole("banner");
    const pill = within(banner).getByRole("status");
    expect(pill, "the pill says Live in words").toHaveTextContent("Live");
    expect(pill).toHaveAttribute("data-tone", "success");
    const buttons = within(banner).getAllByRole("button");
    const [left, theme] = buttons;
    expect(buttons, "the pill is not a button: still three buttons").toHaveLength(3);
    expect(left?.compareDocumentPosition(pill) ?? 0, "after the left panel button").toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(theme?.compareDocumentPosition(pill) ?? 0, "before the theme button").toBe(Node.DOCUMENT_POSITION_PRECEDING);
  });

  it("D3-SHELL: the pill text follows the language", async () => {
    await mountLive();
    await act(async () => {
      await i18n.changeLanguage("es");
    });
    expect(within(screen.getByRole("banner")).getByRole("status")).toHaveTextContent("En vivo");
  });
});

// D8-PANEL (DS-US-DSK-008 D16, D19): the ⚡ pill of the title bar sums the counted conflicts of every repo.
describe("D8 conflict pill", () => {
  const row = (repoId: string, count: number) => ({
    repoId,
    name: repoId,
    path: `/w/${repoId}/.git`,
    state: "observed" as const,
    tier: "active" as const,
    attention: { conflicts: { state: "counted" as const, count }, denials: { state: "counted" as const, count: 0 } },
  });
  const boltPair = (id: string, b = "feat-login") =>
    pair("feat-pagos", b, { pairId: id, rev: 3, state: "current", level: "conflict", computedAtUtcMs: NOW - 40_000, conflictFiles: 1 });

  async function mountWith(selected: string, counts: Record<string, number>, pairs: PairDto[]) {
    const { useEngineStore } = await import("@/state/engine-store");
    useEngineStore.setState({
      conn: { kind: "live" },
      staleSinceMs: null,
      resync: null,
      session: { daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true },
    });
    useUiStore.setState({ selectedRepoId: selected, center: { view: "main" } });
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
    applyEngineBatch(client, [
      {
        type: "global",
        engine: { state: "observing", gitVersion: "2.45.1" },
        daemon: { version: "0.1.0", protocol: 9 },
        autostart: "unknown",
        repos: Object.entries(counts).map(([id, n]) => row(id, n)),
      },
      { type: "repo", fleet: { ...tienda().repo, repoId: "tienda", pairs } },
      { type: "repo", fleet: { ...tienda().repo, repoId: "notas", name: "notas", pairs: [] } },
    ]);
    render(
      <QueryClientProvider client={client}>
        <AppShell />
      </QueryClientProvider>,
    );
  }
  const pill = (name: string | RegExp) => within(screen.getByRole("banner")).getByRole("button", { name });

  it("the conflict pill sums counted conflicts and opens the repo", async () => {
    await mountWith("notas", { tienda: 2, notas: 1 }, [boltPair("a"), boltPair("b", "base")]);
    const button = pill("3 predicted conflicts across your repos. Open them");
    expect(button.textContent, "the number is in text, with the glyph hidden").toContain("3 predicted conflicts");
    expect(button.querySelector('[aria-hidden="true"]')?.textContent).toBe("⚡");
    fireEvent.click(button);
    expect(useUiStore.getState().selectedRepoId, "it opens the repo with the most conflicts").toBe("tienda");
    expect(document.activeElement?.textContent, "with two conflicts there is no single detail: the focus goes to the panel").toBe("Predicted conflicts");
  });

  it("the pill opens the detail when the repo has a single conflict", async () => {
    await mountWith("notas", { tienda: 1, notas: 0 }, [boltPair("only")]);
    fireEvent.click(pill(/predicted conflicts across your repos/));
    expect(useUiStore.getState().selectedRepoId).toBe("tienda");
    expect(useUiStore.getState().center, "a single ⚡ pair opens its detail").toEqual({ view: "pair", repoId: "tienda", pairId: "only" });
  });

  it("the pill is absent without conflicts", async () => {
    await mountWith("tienda", { tienda: 0, notas: 0 }, []);
    expect(within(screen.getByRole("banner")).queryByRole("button", { name: /predicted conflicts/ }), "no ⚡ pill at zero").toBeNull();
    cleanup();
    await mountWith("tienda", { tienda: 1, notas: 0 }, [boltPair("only")]);
    expect(pill(/predicted conflicts across your repos/), "and present with one").toBeInTheDocument();
  });

  it("the pill keeps its full name when compact", async () => {
    setViewportWidth(900);
    await mountWith("tienda", { tienda: 2, notas: 1 }, [boltPair("a"), boltPair("b", "base")]);
    const button = pill("3 predicted conflicts across your repos. Open them");
    expect(button.textContent, "below 1000 px only ⚡ N").toBe("⚡ 3");
    await act(async () => void (await i18n.changeLanguage("es")));
    expect(pill("3 conflictos previstos en tus repos. Abrirlos"), "the accessible name follows the language").toBeInTheDocument();
  });
});
