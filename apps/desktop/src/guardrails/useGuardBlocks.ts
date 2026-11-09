import type { UseQueryResult } from "@tanstack/react-query";
import type { GuardBlocks } from "@/state/engine-cache";

/** SKELETON: mounted once by `EngineCenter`; rereads `guard_blocks` every 10 s while visible (D3). */
export function useGuardBlocks(): Pick<UseQueryResult<GuardBlocks>, "data" | "isPending"> {
  return { data: undefined, isPending: true };
}
