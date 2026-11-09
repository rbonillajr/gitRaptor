// D3-SHELL (DS-US-DSK-003 T007): the status pill sits in the title bar between the left panel button and the theme
// button. It is checked through the window (`AppShell`), which wires the title bar to the engine store and the cache;
// the plan does not say whether `TitleBar` takes the pill as a prop or reads it, so the test does not guess.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
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
