import { invoke } from "@tauri-apps/api/core";
import type { CommandName, Commands } from "./commands";

/** A command the backend rejected. `code` is the kebab-case `CommandError` code (`invalid-params`…). */
export class CommandFailure extends Error {
  readonly code: string;

  constructor(code: string) {
    super(`command failed: ${code}`);
    this.name = "CommandFailure";
    this.code = code;
  }
}

function codeOf(error: unknown): string {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error.code;
  }
  return "unknown";
}

/**
 * Calls a backend command with its params wrapped as `{ params }` (D3). The only file that imports
 * `@tauri-apps/api/core` (V5, ADR-GRP-017).
 */
export async function call<C extends CommandName>(name: C, params: Commands[C]["params"]): Promise<Commands[C]["result"]> {
  try {
    return await invoke<Commands[C]["result"]>(name, { params });
  } catch (error) {
    throw new CommandFailure(codeOf(error));
  }
}
