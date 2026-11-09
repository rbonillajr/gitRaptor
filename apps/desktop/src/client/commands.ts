/**
 * The closed list of IPC commands of the backend and their types. Must equal
 * `src-tauri/commands.json` (D4); `commands.test.ts` checks it.
 */
export const COMMANDS = ["app_info"] as const;

export type CommandName = (typeof COMMANDS)[number];

/** Result of `app_info`: the app version and the channel protocol it speaks. */
export interface AppInfo {
  version: string;
  protocol: number;
}

/** Params and result of each command. */
export interface Commands {
  app_info: { params: Record<string, never>; result: AppInfo };
}
