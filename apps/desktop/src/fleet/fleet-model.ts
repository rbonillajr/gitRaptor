import type { TFunction } from "i18next";
import type { PairDto, RepoFleet, SessionDto, WorktreeDto } from "@/client/engine-types";

export type Group = "main" | "attention" | "active" | "inactive" | "ended" | "no-agent";

/** D4: the order the groups appear in. */
export const GROUP_ORDER: readonly Group[] = ["main", "attention", "active", "inactive", "ended", "no-agent"];

export interface CardModel {
  key: string;
  folder: string;
  group: Group;
  main: boolean;
  /** `●` active, `◐` inactive, `○` ended or no agent, `⚠` unavailable worktree, `""` agent not available. */
  symbol: string;
  agent: {
    label: string;
    /** Filled by the view from `assignSlots`; the model has no memory between renders. */
    colorSlot: number | null;
    /** Session whose lane colour the name takes; `null` when the card shows no live agent. */
    sessionId: string | null;
    muted: boolean;
    shared: boolean;
  };
  /** What the card knows of its agent: a present state, an `ended` one inside 24 h, none, or unknown detection. */
  agentState: "active" | "inactive" | "ended" | "none" | "unavailable";
  stateText: string;
  /** `null` for an unavailable worktree. */
  branchText: string | null;
  /** The 7-digit hash that opens `branchText` of a detached HEAD (rendered as `<code>`). */
  branchCommit: string | null;
  filesCount: number | null;
  filesText: string | null;
  syncText: string | null;
  activityText: string | null;
  attention: { kind: "conflict"; text: string; conflictWith: Array<string | "base"> } | null;
  unavailableText: string | null;
  a11yLabel: string;
}

/** D5: how long an ended session still shows on its card. */
export const ENDED_WINDOW_MS = 24 * 60 * 60 * 1000;
const HASH_DIGITS = 7;

/** "2 min" with the largest whole unit (seconds, minutes, hours, days). */
export function formatAge(ms: number, t: TFunction): string {
  const seconds = Math.floor(Math.max(0, ms) / 1000);
  if (seconds < 60) return t("age.seconds", { count: seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t("age.minutes", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("age.hours", { count: hours });
  return t("age.days", { count: Math.floor(hours / 24) });
}

/** Keys on the other side of every pair that includes `key` with a known conflict (`current` or `recalculating`). */
export function knownConflicts(pairs: readonly PairDto[], key: string): Array<string | "base"> {
  const others: Array<string | "base"> = [];
  for (const p of pairs) {
    if (p.level !== "conflict" || (p.state !== "current" && p.state !== "recalculating")) continue;
    const other = p.a === key ? p.b : p.b === key ? p.a : null;
    if (other !== null && !others.includes(other)) others.push(other);
  }
  return others;
}

const presentRank = (s: SessionDto) => (s.state === "active" ? 0 : 1);

/** Present sessions first by state, then the most recent change, then id (DS-US-CKP-001 § 2). */
function comparePresent(a: SessionDto, b: SessionDto): number {
  return presentRank(a) - presentRank(b) || b.stateSinceUtcMs - a.stateSinceUtcMs || cmp(a.sessionId, b.sessionId);
}

const endedAt = (s: SessionDto) => s.endedUtcMs ?? s.stateSinceUtcMs;
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

const agentName = (s: SessionDto, t: TFunction) => (s.agent.kind === "claude-code" ? t("fleet.agent.claudeCode") : s.agent.name);

interface AgentView {
  group: Extract<Group, "active" | "inactive" | "ended" | "no-agent">;
  symbol: string;
  label: string;
  sessionId: string | null;
  muted: boolean;
  shared: boolean;
  state: CardModel["agentState"];
  stateText: string;
}

function agentView(worktree: WorktreeDto, sessions: readonly SessionDto[], detection: boolean | null, nowMs: number, t: TFunction): AgentView {
  const mine = sessions.filter((s) => s.worktreeKey === worktree.key);
  const present = mine.filter((s) => s.state !== "ended").sort(comparePresent);
  const top = present[0];
  if (top) {
    const more = present.length - 1;
    const named = t("fleet.agent.inWorktree", { agent: agentName(top, t), worktree: worktree.folder });
    return {
      group: top.state === "active" ? "active" : "inactive",
      symbol: top.state === "active" ? "●" : "◐",
      label: more > 0 ? `${named} ${t("fleet.agent.more", { count: more })}` : named,
      sessionId: top.sessionId,
      muted: false,
      shared: present.length >= 2,
      state: top.state === "active" ? "active" : "inactive",
      stateText: t(top.state === "active" ? "fleet.state.active" : "fleet.state.inactive"),
    };
  }

  const last = mine
    .filter((s) => s.state === "ended")
    .sort((a, b) => endedAt(b) - endedAt(a) || cmp(a.sessionId, b.sessionId))[0];
  if (last && nowMs - endedAt(last) < ENDED_WINDOW_MS) {
    const age = formatAge(nowMs - endedAt(last), t);
    const stateText =
      last.endedUtcMs !== null
        ? t("fleet.state.ended", { age })
        : last.endCause === "ended-during-gap"
          ? t("fleet.state.endedUnobserved", { age })
          : t("fleet.state.endTimeUnknown");
    return {
      group: "ended",
      symbol: "○",
      label: t("fleet.agent.inWorktree", { agent: agentName(last, t), worktree: worktree.folder }),
      sessionId: null,
      muted: true,
      shared: false,
      state: "ended",
      stateText,
    };
  }

  if (detection !== true) {
    return { group: "no-agent", symbol: "", label: t("fleet.agentNotAvailable"), sessionId: null, muted: true, shared: false, state: "unavailable", stateText: "" };
  }
  return {
    group: "no-agent",
    symbol: "○",
    label: t("fleet.noAgent", { worktree: worktree.folder }),
    sessionId: null,
    muted: true,
    shared: false,
    state: "none",
    stateText: last ? t("fleet.lastAgent", { agent: agentName(last, t), age: formatAge(nowMs - endedAt(last), t) }) : "",
  };
}

const SYNC_KEY = {
  "base-missing": "fleet.sync.baseMissing",
  "no-base": "fleet.sync.noBase",
  "no-commits": "fleet.sync.noCommits",
  unreadable: "fleet.sync.unreadable",
} as const;

/**
 * One card per worktree in the D4 order. Pure: the clock, the detection flag (`activity` of the session
 * info) and the translator come in; nothing is remembered between calls.
 *
 * @param activity `SessionInfo.activity`: without it no activity time is known, so the cell says "–".
 */
export function fleetCards(fleet: RepoFleet, nowMs: number, activity: boolean, t: TFunction): CardModel[] {
  const { repo } = fleet;
  const sessions = [...fleet.sessions.values()];
  const baseName = repo.base.name ?? "base";
  const folderOf = (key: string) => repo.worktrees.find((w) => w.key === key)?.folder ?? key;

  const cards = repo.worktrees.map((w) => {
    const agent = agentView(w, sessions, fleet.detection, nowMs, t);
    const conflictWith = knownConflicts(repo.pairs, w.key);
    const group: Group = w.main ? "main" : conflictWith.length > 0 ? "attention" : agent.group;

    const ready = w.status.state === "ready" ? w.status : null;
    let branchText: string | null = null;
    let branchCommit: string | null = null;
    let filesCount: number | null = null;
    let filesText: string | null = null;
    let syncText: string | null = null;
    if (ready) {
      if (ready.head.kind === "branch") branchText = ready.head.name;
      else if (ready.head.kind === "unborn") branchText = t("fleet.branch.unborn", { name: ready.head.name });
      else if (w.detachedAt !== null && w.detachedAt.length > 0) {
        branchCommit = w.detachedAt.slice(0, HASH_DIGITS);
        branchText = t("fleet.branch.noBranch", { commit: branchCommit });
      } else branchText = t("fleet.branch.noBranchUnknown");

      filesCount = ready.counts.staged + ready.counts.unstaged + ready.counts.untracked;
      filesText = filesCount === 0 ? t("fleet.clean") : t("fleet.files", { count: filesCount });

      const d = ready.divergence;
      syncText =
        d.state === "counted"
          ? t("fleet.sync.label", {
              ahead: `${d.ahead.count}${d.ahead.exact ? "" : "+"}`,
              behind: `${d.behind.count}${d.behind.exact ? "" : "+"}`,
              base: baseName,
            })
          : t(SYNC_KEY[d.state]);
    }

    const unavailableText = w.status.state === "unavailable" ? t(`fleet.unavailable.${w.status.reason}`) : null;
    const activityText =
      ready === null
        ? null
        : activity && w.lastActivityUtcMs !== null
          ? t("fleet.activity.ago", { age: formatAge(nowMs - w.lastActivityUtcMs, t) })
          : t("fleet.activity.unknown");

    const attentionText =
      conflictWith.length === 0
        ? null
        : conflictWith.length > 1
          ? t("fleet.attention.conflictMany", { count: conflictWith.length })
          : conflictWith[0] === "base"
            ? t("fleet.attention.conflictBase", { base: baseName })
            : t("fleet.attention.conflict", { other: folderOf(conflictWith[0] ?? "") });

    const agentText = agent.shared ? `${agent.label} ${t("fleet.agent.shared")}` : agent.label;
    const a11yLabel = [
      w.main ? t("fleet.main") : null,
      agentText,
      agent.stateText,
      unavailableText,
      branchText,
      filesText,
      syncText,
      activityText,
      attentionText?.replace(/^⚡\s*/, "") ?? null,
    ]
      .filter((p): p is string => p !== null && p !== "")
      .join(", ");

    const card: CardModel = {
      key: w.key,
      folder: w.folder,
      group,
      main: w.main,
      symbol: unavailableText !== null ? "⚠" : agent.symbol,
      agent: { label: agent.label, colorSlot: null, sessionId: agent.sessionId, muted: agent.muted, shared: agent.shared },
      agentState: agent.state,
      stateText: agent.stateText,
      branchText,
      branchCommit,
      filesCount,
      filesText,
      syncText,
      activityText,
      attention: attentionText === null ? null : { kind: "conflict", text: attentionText, conflictWith },
      unavailableText,
      a11yLabel,
    };
    return { card, path: w.path, folder: w.folder };
  });

  return cards
    .sort(
      (a, b) =>
        GROUP_ORDER.indexOf(a.card.group) - GROUP_ORDER.indexOf(b.card.group) ||
        cmp(a.folder, b.folder) ||
        cmp(a.path, b.path) ||
        cmp(a.card.key, b.card.key),
    )
    .map((c) => c.card);
}
