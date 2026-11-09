import type { TFunction } from "i18next";
import type { ConfirmCommandDto, ConflictFileDto, LimitCode, PairDetailDto, PairDto, PairReason, RepoFleet } from "@/client/engine-types";
import { formatAge, type CardModel } from "@/fleet/fleet-model";

export type SideRef = { kind: "worktree"; key: string } | { kind: "base" };

export interface ConflictBandModel {
  pairId: string;
  level: "conflict" | "overlap";
  a: SideRef;
  b: SideRef;
  aLabel: string;
  bLabel: string;
  firstFile: string | null;
  moreText: string | null;
  freshnessText: string;
  stale: boolean;
  isNew: boolean;
  a11yLabel: string;
}

export interface BasePending {
  base: string;
  repo: string;
  command: ConfirmCommandDto;
}

export interface PredictionViewModel {
  bands: ConflictBandModel[];
  lines: string[];
  basePending: BasePending | null;
  limits: LimitCode[];
}

export interface PairDetailViewModel {
  status: "default" | "overlap-only" | "against-base" | "loading" | "stale" | "calculating" | "pending" | "resolved" | "gone";
  title: string;
  subtitle: string;
  freshnessText: string;
  conflictFiles: Array<{ path: string; kindText: string; aText: string; bText: string; mergeRule: boolean }>;
  overlapFiles: string[];
  truncated: boolean;
  basePending: BasePending | null;
}

const REASON_KEY = {
  "base-unconfirmed": "conflicts.reason.baseUnconfirmed",
  "operation-in-progress": "conflicts.reason.operationInProgress",
  "power-saving": "conflicts.reason.powerSaving",
  dormant: "conflicts.reason.dormant",
  "base-missing": "conflicts.reason.baseMissing",
  "missing-object": "conflicts.reason.missingObject",
  shallow: "conflicts.reason.shallow",
  "limit-exceeded": "conflicts.reason.limitExceeded",
  error: "conflicts.reason.error",
} as const satisfies Record<PairReason, string>;
const KIND_KEY = {
  content: "conflicts.kind.content",
  "add-add": "conflicts.kind.addAdd",
  "modify-delete": "conflicts.kind.modifyDelete",
  rename: "conflicts.kind.rename",
  "directory-file": "conflicts.kind.directoryFile",
  binary: "conflicts.kind.binary",
  submodule: "conflicts.kind.submodule",
} as const satisfies Record<ConflictFileDto["kind"], string>;
const isBase = (key: string) => key === "base";
const hasResult = (p: PairDto) => (p.state === "current" || p.state === "recalculating") && p.computedAtUtcMs !== null;
const isBand = (p: PairDto) => hasResult(p) && (p.level === "conflict" || p.level === "overlap");
const byId = (x: PairDto, y: PairDto) => (x.pairId < y.pairId ? -1 : x.pairId > y.pairId ? 1 : 0);

/** The detail of a pair, only when it belongs to the rev of the summary (D4). */
function detailOf(fleet: RepoFleet, p: PairDto): PairDetailDto | null {
  const d = fleet.details.get(p.pairId);
  return d !== undefined && d.pair.rev === p.rev ? d : null;
}

const sideRef = (key: string): SideRef => (isBase(key) ? { kind: "base" } : { kind: "worktree", key });

function freshness(p: PairDto, nowMs: number, t: TFunction): string {
  if (p.computedAtUtcMs === null) return "";
  const age = formatAge(nowMs - p.computedAtUtcMs, t);
  return t(p.state === "recalculating" ? "conflicts.freshness.stale" : "conflicts.freshness.ago", { age });
}

/** D8: the label of the agent card, else the folder; the base says which branch it is. */
function sideName(fleet: RepoFleet, cards: readonly CardModel[], key: string, t: TFunction): string {
  if (isBase(key)) return t("conflicts.side.base", { base: fleet.repo.base.name ?? "base" });
  return cards.find((c) => c.key === key)?.agent.label ?? fleet.repo.worktrees.find((w) => w.key === key)?.folder ?? key;
}

/**
 * D11, G2: the confirm warning shows when a pair says `base-unconfirmed`, or when the snapshot says the base is
 * unconfirmed and no pair against the base has left that state (a computed pair proves the base is confirmed).
 */
function basePendingOf(fleet: RepoFleet): BasePending | null {
  const { repo } = fleet;
  const waiting = (p: PairDto) => p.state === "pending" && p.reason === "base-unconfirmed";
  const show =
    repo.pairs.some(waiting) ||
    (repo.base.status === "unconfirmed" && repo.pairs.filter((p) => isBase(p.a) || isBase(p.b)).every(waiting));
  return show ? { base: repo.base.name ?? "base", repo: repo.name, command: repo.confirmCommand } : null;
}

/** Pure rules of DS-US-DSK-008 D8-D11: bands, freshness lines, pending base. `isNew` comes from the `AlertState`. */
export function predictionView(
  fleet: RepoFleet,
  cards: readonly CardModel[],
  nowMs: number,
  t: TFunction,
  isNew: (pairId: string) => boolean,
): PredictionViewModel {
  const { repo } = fleet;

  const bands = repo.pairs
    .filter(isBand)
    .sort((x, y) => Number(y.level === "conflict") - Number(x.level === "conflict") || byId(x, y))
    .map((p): ConflictBandModel => {
      const detail = detailOf(fleet, p);
      const conflict = p.level === "conflict";
      const firstFile = (conflict ? detail?.conflict?.files[0]?.path : detail?.overlap[0]) ?? null;
      const extra = (conflict ? p.conflictFiles : p.overlapFiles) - 1;
      const truncated = conflict ? p.conflictTruncated : p.overlapPartial;
      const moreText =
        extra > 0 ? t("conflicts.band.more", { count: extra }) + (truncated ? "+" : "") : truncated ? t("conflicts.band.moreTruncated") : null;
      const freshnessText = freshness(p, nowMs, t);
      const aLabel = sideName(fleet, cards, p.a, t);
      const bLabel = sideName(fleet, cards, p.b, t);
      const a11yLabel = t("conflicts.band.a11y", {
        level: t(conflict ? "conflicts.a11y.conflict" : "conflicts.a11y.overlap"),
        a: aLabel,
        b: bLabel,
        file: firstFile ?? "",
        freshness: freshnessText,
      }).replace(/,\s*,/, ",");
      return {
        pairId: p.pairId,
        level: conflict ? "conflict" : "overlap",
        a: sideRef(p.a),
        b: sideRef(p.b),
        aLabel,
        bLabel,
        firstFile,
        moreText,
        freshnessText,
        stale: p.state === "recalculating",
        isNew: isNew(p.pairId),
        a11yLabel,
      };
    });

  const lines: string[] = [];
  const total = repo.pairs.length;
  const calculating = repo.pairs.filter((p) => p.state === "calculating" || ((p.state === "current" || p.state === "recalculating") && p.computedAtUtcMs === null));
  if (calculating.length > 0) {
    // D10: the most recent result the app saw before the engine dropped it; its level is not shown.
    const last = Math.max(-1, ...calculating.map((p) => fleet.lastComputed.get(p.pairId) ?? -1));
    lines.push(
      last >= 0
        ? t("conflicts.freshness.calculatingSince", { n: calculating.length, count: total, age: formatAge(nowMs - last, t) })
        : t("conflicts.freshness.calculating", { n: calculating.length, count: total }),
    );
  }
  for (const [state, key] of [
    ["pending", "conflicts.freshness.pending"],
    ["not-computable", "conflicts.freshness.notComputable"],
  ] as const) {
    const byReason = new Map<PairReason, number>();
    for (const p of repo.pairs) if (p.state === state && p.reason !== null) byReason.set(p.reason, (byReason.get(p.reason) ?? 0) + 1);
    for (const [reason, count] of byReason) lines.push(t(key, { reason: t(REASON_KEY[reason]), count }));
  }
  // BR-CKP-CALC-003: "no conflicts" only when every pair has a current result and none is a conflict or overlap.
  const clear = total > 0 && !repo.pairsTruncated && repo.pairs.every((p) => p.state === "current" && p.computedAtUtcMs !== null && p.level === "none");
  if (clear) {
    const oldest = Math.min(...repo.pairs.map((p) => p.computedAtUtcMs ?? nowMs));
    lines.push(t("conflicts.freshness.clear", { age: formatAge(nowMs - oldest, t) }));
  }

  return { bands, lines, basePending: basePendingOf(fleet), limits: repo.limits };
}

/** One range as text: `line 7` for a length of 0 or 1, `lines 40–58` otherwise. */
function rangeText(start: number, len: number, t: TFunction): string {
  return len <= 1 ? t("conflicts.range.line", { start }) : t("conflicts.range.lines", { start, end: start + len - 1 });
}

/** The detail of one pair with its files and ranges (DS-US-DSK-008 T004, T006). */
export function pairDetailView(
  fleet: RepoFleet,
  cards: readonly CardModel[],
  pairId: string,
  nowMs: number,
  t: TFunction,
): PairDetailViewModel {
  const { repo } = fleet;
  const p = repo.pairs.find((x) => x.pairId === pairId);
  if (p === undefined) {
    return { status: "gone", title: "", subtitle: "", freshnessText: "", conflictFiles: [], overlapFiles: [], truncated: false, basePending: null };
  }

  const baseName = repo.base.name ?? "base";
  const folderOf = (key: string) => (isBase(key) ? baseName : (repo.worktrees.find((w) => w.key === key)?.folder ?? key));
  const aLabel = sideName(fleet, cards, p.a, t);
  const bLabel = sideName(fleet, cards, p.b, t);
  const detail = detailOf(fleet, p);

  let status: PairDetailViewModel["status"];
  if (p.state === "calculating") status = "calculating";
  else if (p.state === "pending" || p.state === "not-computable") status = "pending";
  else if (p.level !== "conflict" && p.level !== "overlap") status = "resolved";
  else if (detail === null) status = "loading";
  else if (p.state === "recalculating") status = "stale";
  else if (p.level === "overlap") status = "overlap-only";
  else status = isBase(p.b) ? "against-base" : "default";

  const withResult = status !== "calculating" && status !== "pending" && status !== "resolved";
  const title = !withResult
    ? t("conflicts.band.pair", { a: aLabel, b: bLabel })
    : p.level === "overlap"
      ? t("conflicts.detail.titleOverlap", { a: aLabel, b: bLabel })
      : isBase(p.b)
        ? t("conflicts.detail.titleBase", { a: aLabel, base: baseName })
        : t("conflicts.detail.title", { a: aLabel, b: bLabel });
  const folders = { a: folderOf(p.a), b: folderOf(p.b) };
  const subtitle =
    p.computedAtUtcMs === null
      ? t("conflicts.band.pair", folders)
      : t("conflicts.detail.computed", { ...folders, age: formatAge(nowMs - p.computedAtUtcMs, t) });

  const sideText = (side: string, ranges: ReadonlyArray<{ start: number; len: number }>, more: boolean) =>
    t("conflicts.detail.side", {
      side,
      ranges: [...ranges.map((r) => rangeText(r.start, r.len, t)), ...(more ? [t("conflicts.range.more")] : [])].join(", "),
    });
  const conflictFiles =
    withResult && detail?.conflict
      ? detail.conflict.files.map((f) => ({
          path: f.path,
          kindText: t(KIND_KEY[f.kind]),
          aText: sideText(aLabel, f.ours, f.hunksTruncated),
          bText: sideText(bLabel, f.theirs, f.hunksTruncated),
          mergeRule: f.hasMergeRule,
        }))
      : [];

  return {
    status,
    title,
    subtitle,
    freshnessText: freshness(p, nowMs, t),
    conflictFiles,
    overlapFiles: withResult ? (detail?.overlap ?? []) : [],
    truncated: withResult && ((detail?.conflict?.truncated ?? false) || p.conflictTruncated || (p.level === "overlap" && p.overlapPartial)),
    basePending: p.reason === "base-unconfirmed" ? { base: baseName, repo: repo.name, command: repo.confirmCommand } : null,
  };
}
