/* eslint-disable @typescript-eslint/no-unused-vars -- skeleton: remove with the implementation */
import type { TFunction } from "i18next";
import type { BlockEntryDto, OperationDto, PendingExceptionDto } from "@/client/engine-types";
import type { GuardBlocks } from "@/state/engine-cache";

/** One row of the Guardrails view (DS-US-DSK-007 T004). */
export interface BlockRow {
  id: string;
  group: "blocks" | "unverified";
  kind: BlockEntryDto["kind"];
  title: string;
  nothingHappened: string | null;
  attempted: string;
  why: string | null;
  stoppedBy: string[];
  technical: string[];
  exception: { command: string } | { notOffered: string } | { rebaseForm: string } | null;
  /** `grd.exc.sourceInferred` or `grd.exc.branchDeleteHint`. */
  hint: string | null;
  ctaDisabled: "engine-down" | null;
  a11yLabel: string;
}

/** The last copied exception command of this repo, in memory (D12). */
export type CopiedException = { repoId: string; operation: OperationDto; rules: string[] } | null;

/** SKELETON: the rows, newest first. */
export function blockRows(_blocks: GuardBlocks, _live: boolean, _t: TFunction, _lang: string): BlockRow[] {
  return [];
}

/** SKELETON: the denials newer than `seenAfterMs`. */
export function unseen(_entries: readonly BlockEntryDto[], _seenAfterMs: number): BlockEntryDto[] {
  return [];
}

/** SKELETON: whether the announced exception differs from the copied one (D12). */
export function pendingMismatch(_pending: PendingExceptionDto | null, _copied: CopiedException): boolean {
  return false;
}
