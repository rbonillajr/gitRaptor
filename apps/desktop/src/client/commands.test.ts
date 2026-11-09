// FE1 (TS-DSK-001 T006): COMMANDS equals src-tauri/commands.json and call("app_info", {}) sends { params: {} }.
import { Channel } from "@tauri-apps/api/core";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { COMMANDS } from "./commands";
import type { EngineMessage } from "./engine-types";
import { call } from "./ipc";

afterEach(() => {
  clearMocks();
});

describe("FE1 client commands", () => {
  it("FE1: COMMANDS equals src-tauri/commands.json", () => {
    const file = fileURLToPath(new URL("../../src-tauri/commands.json", import.meta.url));
    const listed = JSON.parse(readFileSync(file, "utf8")) as string[];
    expect([...COMMANDS].sort(), "frontend list must equal commands.json").toEqual([...listed].sort());
  });

  it('FE1: call("app_info", {}) invokes app_info with { params: {} }', async () => {
    const seen: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, args });
      return { version: "0.0.0", protocol: 1 };
    });
    const info = await call("app_info", {});
    expect(seen, "one invoke expected").toHaveLength(1);
    expect(seen[0]).toEqual({ cmd: "app_info", args: { params: {} } });
    expect(info).toEqual({ version: "0.0.0", protocol: 1 });
  });
});

describe("D3-CMDS-TS engine commands", () => {
  it("D3-CMDS-TS: COMMANDS declares engine_watch and engine_retry", () => {
    expect([...COMMANDS].sort(), "the closed list gains exactly the two engine commands").toEqual(["app_info", "engine_retry", "engine_watch"]);
  });

  it('D3-CMDS-TS: call("engine_retry", { start }) invokes engine_retry with { params: { start } }', async () => {
    expect(COMMANDS, "engine_retry must be a declared command").toContain("engine_retry");
    const seen: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, args });
      return {};
    });
    await (call as (c: string, p: unknown) => Promise<unknown>)("engine_retry", { start: true });
    expect(seen).toEqual([{ cmd: "engine_retry", args: { params: { start: true } } }]);
  });

  it('D3-CMDS-TS: call("engine_watch", { channel }) invokes engine_watch with { params: { channel } }', async () => {
    expect(COMMANDS, "engine_watch must be a declared command").toContain("engine_watch");
    const seen: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, args });
      return {};
    });
    const channel = new Channel<EngineMessage>();
    await (call as (c: string, p: unknown) => Promise<unknown>)("engine_watch", { channel });
    expect(seen).toEqual([{ cmd: "engine_watch", args: { params: { channel } } }]);
  });
});
