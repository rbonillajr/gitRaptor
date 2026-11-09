import type { Channel } from "@tauri-apps/api/core";
import type { EngineMessage } from "./engine-types";

/**
 * The closed list of IPC commands of the backend and their types. Must equal
 * `src-tauri/commands.json` (D4); `commands.test.ts` checks it.
 */
export const COMMANDS = ["app_info", "engine_watch", "engine_retry", "fleet_open", "guard_blocks"] as const;

export type CommandName = (typeof COMMANDS)[number];

/** Result of `app_info`: the app version and the channel protocol it speaks. */
export interface AppInfo {
  version: string;
  protocol: number;
}

/** Params and result of each command. */
export interface Commands {
  app_info: { params: Record<string, never>; result: AppInfo };
  /** Opens the engine stream: the backend pushes `EngineMessage`s through `channel`. */
  engine_watch: { params: { channel: Channel<EngineMessage> }; result: Record<string, never> };
  /** `start: true` asks the bundled launcher to start the engine; `false` only retries the connection now. */
  engine_retry: { params: { start: boolean }; result: Record<string, never> };
  /** Opens the repo scope in the backend: its snapshot and events then arrive as `repo*` messages (one repo at a time). */
  fleet_open: { params: { repoId: string }; result: Record<string, never> };
  /**
   * Asks the backend to read `guard.status` and `guard.log` of the open repo (read only): the result arrives as
   * `guardBlocks` or `guardBlocksUnavailable`. The path never leaves the backend.
   */
  guard_blocks: { params: { repoId: string }; result: Record<string, never> };
}
