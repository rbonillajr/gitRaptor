/** Shared types of the lane graph (DS-US-DSK-005 § Contratos compartidos). Types only. */
import type { BoundedCountDto } from "@/client/engine-types";
import type { GraphEntry } from "@/state/engine-cache";

export type { GraphEntry };

export type GraphColumnId = "branch" | "graph" | "message" | "author" | "date" | "sha";

/** `laneKey` is the worktree key, or `"base"`. Every row carries the sentence that names it for assistive tech. */
export type GraphRow = { a11yLabel: string } & (
  | { kind: "wip"; id: string; laneKey: string; files: number }
  | { kind: "base"; id: "base"; name: string }
  | { kind: "lane"; id: string; laneKey: string; syncText: string; note: string }
  | { kind: "commit"; id: string; laneKey: string; oid: string; summary: string; committedUtcMs: number; merge: boolean; head: boolean }
  | { kind: "more"; id: string; laneKey: string; count: number; exact: boolean }
  | { kind: "fork"; id: string; laneKey: string; mergeBase: string }
);

export interface LaneModel {
  key: string;
  /** Stable column index (D5): ordered by folder, path and key. */
  column: number;
  branchText: string;
  agentLabel: string;
  colorSlot: number | null;
  muted: boolean;
  symbol: string;
  stateText: string;
  syncText: string;
  shown: number;
  total: BoundedCountDto | null;
}

export interface GraphModel {
  state: "loading" | "ready" | "base-unconfirmed" | "no-base" | "base-missing" | "repo-unavailable";
  baseName: string | null;
  lanes: LaneModel[];
  rows: GraphRow[];
  empty: boolean;
  note: "unsupported" | "failed" | "cut" | null;
}

export interface GraphColors {
  base: string;
  /** The seven `--gr-lane-N` values. */
  lane: readonly string[];
  muted: string;
  danger: string;
}

export interface GraphGeometry {
  rowHeight: number;
  graphWidth: number;
  laneX: ReadonlyMap<string, number>;
  laneSpan: ReadonlyMap<string, { first: number; last: number }>;
  rowIndex: ReadonlyMap<string, number>;
  nodeRadius: number;
  clipped: number;
}

export interface GraphLayer {
  id: string;
  draw(ctx: CanvasRenderingContext2D, geometry: GraphGeometry, window: { first: number; last: number }, colors: GraphColors): void;
}
