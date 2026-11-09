import { queryOptions } from "@tanstack/react-query";
import { call } from "./ipc";

/** Version and protocol of the app backend. */
export const appInfoQuery = queryOptions({
  queryKey: ["app", "info"],
  queryFn: () => call("app_info", {}),
});

/**
 * Keys of the engine data. Entries are only ever written with `setQueryData` by `applyEngineBatch`: there is no
 * network `queryFn` (ADR-GRP-004, Enmienda D7). `repo` is for US-DSK-004.
 */
export const engineKeys = {
  global: ["engine", "global"] as const,
  discovered: ["engine", "discovered"] as const,
  repo: (id: string) => ["engine", "repo", id] as const,
};
