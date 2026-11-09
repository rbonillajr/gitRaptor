// FE1 (TS-DSK-001 T006): COMMANDS equals src-tauri/commands.json and call("app_info", {}) sends { params: {} }.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { COMMANDS } from "./commands";
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
