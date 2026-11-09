// D3-SHELL (DS-US-DSK-003 T007): App starts the engine stream once on mount, marks gr:first-meaningful-paint once on
// the first final state (NFR-13.6), and the retry button calls engine_retry and stays aria-disabled until the next conn.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, type MockInstance, vi } from "vitest";
import "@/i18n";
import type { ConnState, EngineMessage } from "@/client/engine-types";
import { queryClient } from "@/state/query-client";
import { App } from "./App";

const GLOBAL: Extract<EngineMessage, { type: "global" }> = {
  type: "global",
  engine: { state: "observing", gitVersion: "2.45.1" },
  daemon: { version: "0.1.0", protocol: 9 },
  autostart: "unknown",
  repos: [],
};
const SESSION: EngineMessage = { type: "session", daemonVersion: "0.1.0", protocol: 9, restricted: false, activity: true, discovery: true };
const conn = (state: ConnState, staleSinceMs: number | null = null): EngineMessage => ({ type: "conn", state, staleSinceMs, resync: null });

let invoked: { cmd: string; args: unknown }[] = [];
/** The Tauri `Channel` the App passed to engine_watch: only its handler matters here. */
type WatchedChannel = { onmessage: (message: EngineMessage) => void };
let channel: WatchedChannel | undefined;
let marks: MockInstance<typeof performance.mark>;

const calls = (cmd: string) => invoked.filter((i) => i.cmd === cmd);
const ready = () => waitFor(() => expect(channel, "App must call engine_watch with a Channel").toBeDefined());
const paintMarks = () => marks.mock.calls.filter(([name]) => name === "gr:first-meaningful-paint");

/** Delivers a batch as the backend would; the stream hands it to the store at the next frame. */
function send(...batch: EngineMessage[]) {
  act(() => {
    for (const message of batch) channel?.onmessage(message);
  });
}

beforeEach(async () => {
  invoked = [];
  channel = undefined;
  queryClient.clear();
  const { useEngineStore } = await import("@/state/engine-store");
  useEngineStore.setState({ conn: { kind: "connecting" }, staleSinceMs: null, resync: null, session: null });
  marks = vi.spyOn(performance, "mark");
  mockIPC((cmd, args) => {
    invoked.push({ cmd, args });
    if (cmd === "engine_watch") channel = (args as { params: { channel: WatchedChannel } }).params.channel;
    if (cmd === "app_info") return { version: "0.0.0", protocol: 9 };
    return {};
  });
  await act(async () => {
    await i18n.changeLanguage("en");
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("D3-SHELL App", () => {
  it("D3-SHELL: App calls engine_watch once on mount, with a Channel", async () => {
    render(<App />);
    await ready();
    expect(typeof channel?.onmessage, "the Channel handler").toBe("function");
    await screen.findByRole("banner");
    expect(calls("engine_watch"), "engine_watch is called once per webview load").toHaveLength(1);
  });

  it("D3-SHELL: marks gr:first-meaningful-paint once, on a final state and not before", async () => {
    render(<App />);
    await ready();
    send(conn({ kind: "connecting" }));
    await screen.findByText("Connecting to the engine…");
    expect(paintMarks(), "connecting is not a final state").toHaveLength(0);

    send(SESSION, GLOBAL, conn({ kind: "live" }));
    await screen.findByText("Live");
    await waitFor(() => expect(paintMarks()).toHaveLength(1));

    send({ type: "engineState", engine: { state: "waiting-for-git", gitVersion: null } });
    await screen.findByText("Git 2.38 or later missing");
    expect(paintMarks(), "later states do not mark again").toHaveLength(1);
  });

  it("D3-SHELL: a stopped engine is a final state too", async () => {
    render(<App />);
    await ready();
    send(conn({ kind: "stopped", reason: "not-running" }));
    await screen.findByRole("heading", { name: "Engine stopped" });
    await waitFor(() => expect(paintMarks()).toHaveLength(1));
  });

  it("D3-SHELL: Retry now calls engine_retry and stays aria-disabled until the next conn", async () => {
    render(<App />);
    await ready();
    send(SESSION, GLOBAL, conn({ kind: "reconnecting", attempt: 1, retryInMs: 2000 }, Date.now()));
    const retry = await screen.findByRole("button", { name: "Retry now" });
    expect(retry).not.toHaveAttribute("aria-disabled", "true");

    fireEvent.click(retry);
    expect(calls("engine_retry")).toEqual([{ cmd: "engine_retry", args: { params: { start: false } } }]);
    expect(screen.getByRole("button", { name: "Retry now" }), "disabled while the retry is pending").toHaveAttribute("aria-disabled", "true");

    send(conn({ kind: "reconnecting", attempt: 2, retryInMs: 4000 }, Date.now()));
    await waitFor(() => expect(screen.getByRole("button", { name: "Retry now" })).not.toHaveAttribute("aria-disabled", "true"));
  });
});
