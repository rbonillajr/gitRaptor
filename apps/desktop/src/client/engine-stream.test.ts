// D3-STREAM (DS-US-DSK-003 T006): watchEngine hands the messages of the Channel to the store in one
// batch per frame, in order, with a single requestAnimationFrame (NFR-13.1).
import { Channel } from "@tauri-apps/api/core";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { watchEngine } from "./engine-stream";
import type { EngineMessage } from "./engine-types";

let frames: FrameRequestCallback[] = [];

beforeEach(() => {
  frames = [];
  const raf = (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  };
  for (const target of new Set<Window & typeof globalThis>([window, globalThis as unknown as Window & typeof globalThis])) {
    vi.spyOn(target, "requestAnimationFrame").mockImplementation(raf);
    vi.spyOn(target, "cancelAnimationFrame").mockImplementation(() => {});
  }
});

afterEach(() => {
  clearMocks();
  vi.restoreAllMocks();
});

describe("D3-STREAM watchEngine", () => {
  it("D3-STREAM: delivers 1,000 messages in one batch, in order, with one requestAnimationFrame", async () => {
    const invoked: { cmd: string; args: unknown }[] = [];
    let channel: Channel<EngineMessage> | undefined;
    mockIPC((cmd, args) => {
      invoked.push({ cmd, args });
      if (cmd === "engine_watch") channel = (args as { params: { channel: Channel<EngineMessage> } }).params.channel;
      return {};
    });
    const batches: EngineMessage[][] = [];
    await watchEngine((batch: EngineMessage[]) => batches.push(batch));

    expect(
      invoked.map((i) => i.cmd),
      "watchEngine must call engine_watch exactly once",
    ).toEqual(["engine_watch"]);
    expect(channel, "engine_watch must receive a Tauri Channel in params.channel").toBeInstanceOf(Channel);

    const sent: EngineMessage[] = Array.from({ length: 1000 }, (_, i) => ({ type: "repoRetired", repoId: `repo-${i}` }));
    for (const message of sent) channel?.onmessage(message);

    expect(frames, "1,000 messages must schedule a single requestAnimationFrame").toHaveLength(1);
    expect(batches, "nothing is delivered before the frame").toHaveLength(0);

    frames[0]?.(0);
    expect(batches, "the whole batch is delivered at the frame").toHaveLength(1);
    expect(batches[0], "the batch keeps the order of arrival").toEqual(sent);
  });
});

// D4-FRESH (DS-US-DSK-004 T006): each batch carries `receivedAt`, the `performance.now()` of its first message.
// `receivedAt` travels as the second argument of `onBatch(messages, { receivedAt })`.
describe("D4-FRESH watchEngine receivedAt", () => {
  it("receivedAt: each batch carries the performance.now of its first message, not of the last", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    let channel: Channel<EngineMessage> | undefined;
    mockIPC((cmd, args) => {
      if (cmd === "engine_watch") channel = (args as { params: { channel: Channel<EngineMessage> } }).params.channel;
      return {};
    });
    const delivered: unknown[][] = [];
    await (watchEngine as (onBatch: (...args: unknown[]) => void) => Promise<void>)((...args) => delivered.push(args));
    const receivedAt = (args: unknown[] | undefined): unknown => (args?.[1] as { receivedAt?: unknown } | undefined)?.receivedAt;

    now = 100;
    channel?.onmessage({ type: "repoRetired", repoId: "a" });
    now = 150;
    channel?.onmessage({ type: "repoRetired", repoId: "b" });
    frames[0]?.(0);
    expect(delivered, "the first batch is delivered at its frame").toHaveLength(1);
    expect(receivedAt(delivered[0]), "the first message of the batch arrived at 100").toBe(100);

    now = 300;
    channel?.onmessage({ type: "repoRetired", repoId: "c" });
    now = 320;
    frames[1]?.(0);
    expect(delivered, "the second batch is delivered at its frame").toHaveLength(2);
    expect(receivedAt(delivered[1]), "the second batch has its own receivedAt").toBe(300);
    expect((delivered[0]?.[0] as unknown[]).length, "the batch is still the messages in order").toBe(2);
  });
});
