// DSK002-UI (TS-DSK-002 T007, D-A): AutostartSection is the container AppShell mounts under EngineBand. It asks
// `engine_autostart_offer` only while the engine is connected and shows AutostartOffer only for the offers
// `offered` and `registered`; pressing the action invokes `engine_autostart` with `{ enable }`, and a failed
// choice shows the translated error (`not-offered`, `cancelled`, `failed`). The dialog itself is the backend's.
//
// Tested through AppShell (style of AppShell.test.tsx and App.test.tsx), so the test names no prop of the
// container. The control is found by the accessible name AutostartOffer gives it when rendered alone.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import en from "@/i18n/locales/en.json";
import type { ConnState } from "@/client/engine-types";
import { useEngineStore } from "@/state/engine-store";
import { useUiStore } from "@/state/ui-store";
import { setViewportWidth } from "../../tests/setup";
import { AppShell } from "@/shell/AppShell";
import { AutostartOffer } from "./AutostartOffer";

type Offer = { offer: "offered" | "registered" | "not-offered"; reason: "location" | "other-binary" | null };

let invoked: { cmd: string; args: unknown }[] = [];
const calls = (cmd: string) => invoked.filter((i) => i.cmd === cmd);

function ipc(offer: Offer, onAutostart: (enable: boolean) => unknown = (enable) => ({ registered: enable })) {
  mockIPC((cmd, args) => {
    invoked.push({ cmd, args });
    if (cmd === "app_info") return { version: "0.0.0", protocol: 9 };
    if (cmd === "engine_autostart_offer") return offer;
    if (cmd === "engine_autostart") return onAutostart((args as { params: { enable: boolean } }).params.enable);
    return {};
  });
}

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <AppShell />
    </QueryClientProvider>,
  );
}

/** The accessible name AutostartOffer gives its control for `offer`, rendered alone. */
function actionName(offer: "offered" | "registered"): string {
  const view = render(<AutostartOffer offer={offer} onChange={() => {}} />);
  const control = [...screen.queryAllByRole("button"), ...screen.queryAllByRole("switch")][0] as HTMLElement;
  const name = control.textContent?.trim() || control.getAttribute("aria-label") || "";
  view.unmount();
  return name;
}

const setConn = (conn: ConnState) => act(() => useEngineStore.setState({ conn }));
const settle = () => act(() => new Promise<void>((resolve) => setTimeout(resolve, 60)));

beforeEach(async () => {
  invoked = [];
  useUiStore.setState({ theme: "auto", language: "auto", sidebarOpen: true, detailOpen: true });
  useEngineStore.setState({ conn: { kind: "connecting" }, staleSinceMs: null, resync: null, session: null });
  setViewportWidth(1440);
  await act(async () => {
    await i18n.changeLanguage("en");
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("DSK002-UI AutostartSection", () => {
  it("DSK002-UI: with the engine connected and the offer offered, the action to enable is shown and invokes engine_autostart { enable: true }", async () => {
    const name = actionName("offered");
    ipc({ offer: "offered", reason: null });
    mount();
    await setConn({ kind: "live" });
    const button = await screen.findByRole("button", { name }, { timeout: 2000 });
    expect(calls("engine_autostart_offer"), "AutostartSection must ask engine_autostart_offer").not.toHaveLength(0);
    fireEvent.click(button);
    await waitFor(() => expect(calls("engine_autostart")).toHaveLength(1));
    expect(calls("engine_autostart")[0]?.args, "pressing offered must send { params: { enable: true } }").toEqual({ params: { enable: true } });
  });

  it("DSK002-UI: with the offer registered, the action to stop is shown and invokes engine_autostart { enable: false }", async () => {
    const name = actionName("registered");
    ipc({ offer: "registered", reason: null });
    mount();
    await setConn({ kind: "live" });
    const button = await screen.findByRole("button", { name }, { timeout: 2000 });
    fireEvent.click(button);
    await waitFor(() => expect(calls("engine_autostart")).toHaveLength(1));
    expect(calls("engine_autostart")[0]?.args, "pressing registered must send { params: { enable: false } }").toEqual({ params: { enable: false } });
  });

  it.each([
    ["location", { offer: "not-offered", reason: "location" }],
    ["other-binary", { offer: "not-offered", reason: "other-binary" }],
  ] as const)("DSK002-UI: a not-offered offer (%s) shows no action", async (_name, offer) => {
    const enable = actionName("offered");
    const stop = actionName("registered");
    ipc(offer);
    mount();
    await setConn({ kind: "live" });
    await waitFor(() => expect(calls("engine_autostart_offer"), "the offer must be asked while connected").not.toHaveLength(0));
    await settle();
    expect(screen.queryByRole("button", { name: enable }), "not-offered must not show the enable action").toBeNull();
    expect(screen.queryByRole("button", { name: stop }), "not-offered must not show the stop action").toBeNull();
  });

  it.each([
    ["connecting", { kind: "connecting" }],
    ["starting", { kind: "starting" }],
    ["stopped", { kind: "stopped", reason: "not-running" }],
    ["reconnecting", { kind: "reconnecting", attempt: 1, retryInMs: 2000 }],
  ] as const)("DSK002-UI: with the engine %s it neither asks the offer nor shows the action", async (_name, conn) => {
    const enable = actionName("offered");
    ipc({ offer: "offered", reason: null });
    mount();
    await setConn(conn);
    await settle();
    expect(calls("engine_autostart_offer"), "the offer must not be asked without a live engine").toHaveLength(0);
    expect(screen.queryByRole("button", { name: enable }), "no action without a live engine").toBeNull();
  });

  it.each(["not-offered", "cancelled", "failed"] as const)("DSK002-UI: the error %s of engine_autostart is shown translated", async (code) => {
    const name = actionName("offered");
    ipc({ offer: "offered", reason: null }, () => Promise.reject({ code }));
    mount();
    await setConn({ kind: "live" });
    fireEvent.click(await screen.findByRole("button", { name }, { timeout: 2000 }));
    const errors = (en as unknown as { error?: Record<string, string> }).error ?? {};
    const camel = code.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase());
    const text = errors[code] ?? errors[camel];
    expect(text, `en.json has no text for the error ${code}`).toBeTruthy();
    expect(await screen.findByText(text as string, { exact: false }), `the error ${code} must be shown with its translation`).toBeInTheDocument();
  });
});
