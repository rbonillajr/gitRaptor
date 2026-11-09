import type { TFunction } from "i18next";
import type { BlockEntryDto, OperationDto, PendingExceptionDto, RefDto } from "@/client/engine-types";
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
  /** The text of `grd.exc.sourceInferred` or `grd.exc.branchDeleteHint`, already worded. */
  hint: string | null;
  ctaDisabled: "engine-down" | null;
  a11yLabel: string;
}

/** The last copied exception command of this repo, in memory (D12). */
export type CopiedException = { repoId: string; operation: OperationDto; rules: string[] } | null;

/** The form shown for a rebase, which has no command to copy (D7). */
export const REBASE_FORM = "raptor guard exec -- git rebase …";

type Tx = (key: string, options?: Record<string, unknown>) => string;
/** Keys built from engine values (rule, level, reason) cannot be checked against the catalog types. */
const tx = (t: TFunction): Tx => t as unknown as Tx;

/** Local clock time of a UTC millisecond value, in the language of the app and the zone of the machine. */
export function formatClock(ms: number, lang: string): string {
  return new Intl.DateTimeFormat(lang, { hour: "numeric", minute: "2-digit" }).format(ms);
}

/** The agent type, never a session name (D5); no actor is `an agent`, never `human`. */
export function agentName(actor: BlockEntryDto["actor"], t: TFunction): string {
  const text = tx(t);
  if (actor === null) return text("grd.agent.none");
  return text(actor.kind === "claude-code" ? "grd.agent.claudeCode" : "grd.agent.other");
}

/** The rule in plain words (`guard.log.rule.*`); an unknown wire rule shows as itself. */
export function ruleText(rule: string, t: TFunction): string {
  const key = rule
    .split(".")
    .map((part) => part.replace(/-(.)/g, (_, c: string) => c.toUpperCase()))
    .join(".");
  return tx(t)(`grd.rule.${key}`, { defaultValue: rule });
}

const names = (refs: readonly RefDto[]) => refs.map((r) => r.name).join(", ");
const refsWith = (refs: readonly RefDto[], change: RefDto["change"]) => refs.filter((r) => r.change === change);

/** The operation in plain words, for a title (D17): a push says `on the server (origin)`. */
export function opPhrase(op: OperationDto, branch: string | null, t: TFunction): string {
  const text = tx(t);
  const unknown = text("grd.unknown");
  switch (op.kind) {
    case "push": {
      const remote = op.remote ?? unknown;
      const force = refsWith(op.refs, "force");
      const del = refsWith(op.refs, "delete");
      if (force.length > 0) return text("grd.op.forcePush", { ref: names(force), remote });
      if (del.length > 0) return text("grd.op.pushDelete", { ref: names(del), remote });
      return text("grd.op.push", { ref: op.refs.length > 0 ? names(op.refs) : unknown, remote });
    }
    case "ref-transaction": {
      const del = refsWith(op.refs, "delete");
      return del.length > 0 ? text("grd.op.branchDelete", { ref: names(del) }) : text("grd.op.refMove", { ref: names(op.refs) });
    }
    case "rebase":
      return branch !== null ? text("grd.op.rebase", { branch }) : text("grd.op.rebaseUnknown");
    case "commit":
      return text("grd.op.commit", { branch: branch ?? unknown });
  }
}

const NOT_OFFERED_KEY: Record<string, string> = {
  "where-unknown": "whereUnknown",
  "rate-limited": "rateLimited",
  "unsafe-text": "unsafeText",
  "raptor-unknown": "raptorUnknown",
};

function attempted(e: BlockEntryDto, t: TFunction, lang: string): string {
  const text = tx(t);
  const time = formatClock(e.atMs, lang);
  const op = e.operation;
  let base: string;
  if (e.kind === "exception-cancelled" && e.actor !== null) {
    base = text("grd.exc.cancelledBy", { actor: agentName(e.actor, t) });
  } else if (op.kind === "push" && e.branch !== null && e.kind === "denial") {
    const target = op.refs.find((r) => r.change !== "delete" && r.name !== e.branch);
    base =
      target === undefined
        ? text("grd.block.attemptedOp", { operation: opPhrase(op, e.branch, t), time })
        : text("grd.block.attemptedPush", {
            kind: text(target.change === "force" ? "grd.kind.forcePush" : "grd.kind.push"),
            source: e.branch,
            ref: target.name,
            remote: op.remote ?? text("grd.unknown"),
            time,
          });
  } else {
    base = text("grd.block.attemptedOp", { operation: opPhrase(op, e.branch, t), time });
  }
  return e.count > 1 ? `${base} ${text("grd.block.times", { count: e.count, time: formatClock(e.lastMs, lang) })}` : base;
}

function titleOf(e: BlockEntryDto, t: TFunction): string {
  const text = tx(t);
  const operation = opPhrase(e.operation, e.branch, t);
  const rules = e.rules.map((r) => ruleText(r.rule, t)).join(", ");
  switch (e.kind) {
    case "exception":
      return text("grd.exc.done", { operation, rules });
    case "exception-cancelled":
      return text("grd.exc.cancelled");
    case "exception-rejected":
      return text("grd.exc.rejected", { operation });
    case "denial":
      return e.actor === null
        ? text("grd.block.titleNoActor", { operation })
        : text("grd.block.title", { actor: agentName(e.actor, t), operation });
  }
}

function exceptionOf(e: BlockEntryDto, t: TFunction): BlockRow["exception"] {
  const x = e.exception;
  if (x === null) return null;
  if (x.state === "offered") return { command: x.command };
  if (x.reason === "rebase") return { rebaseForm: REBASE_FORM };
  return { notOffered: tx(t)(`grd.exc.notOffered.${NOT_OFFERED_KEY[x.reason] ?? x.reason}`) };
}

/** M-04 and M-05: the lines that say what the command guesses or how to force a branch delete on purpose. */
function hintOf(e: BlockEntryDto, t: TFunction): string | null {
  if (e.exception?.state !== "offered") return null;
  const op = e.operation;
  if (op.kind === "push" && e.branch !== null) {
    const target = op.refs.find((r) => r.change !== "delete");
    if (target !== undefined && target.name !== e.branch) return tx(t)("grd.exc.sourceInferred", { branch: e.branch });
  }
  if (op.kind === "ref-transaction" && op.refs.length > 0 && op.refs.every((r) => r.change === "delete")) {
    return tx(t)("grd.exc.branchDeleteHint");
  }
  return null;
}

function rowOf(e: BlockEntryDto, live: boolean, t: TFunction, lang: string): BlockRow {
  const text = tx(t);
  const op = e.operation;
  const denial = e.kind === "denial";
  const refs = op.kind === "push" || op.kind === "ref-transaction" ? op.refs : [];
  const ref = (refsWith(refs, "force")[0] ?? refsWith(refs, "delete")[0] ?? refs[0])?.name;
  const nothingHappened =
    denial && ref !== undefined
      ? text(op.kind === "push" ? "grd.block.nothing" : "grd.block.nothingLocal", { ref: names(refs.filter((r) => r.name === ref)) })
      : null;
  const why = e.rules.some((r) => r.rule === "minimum.force-push")
    ? text("grd.block.why.forcepush")
    : e.rules.some((r) => r.rule === "minimum.base-branch-delete")
      ? text("grd.block.why.deletebase")
      : null;
  const stoppedBy = e.rules.map((r) => `${ruleText(r.rule, t)} (${text(`grd.level.${r.level}`, { defaultValue: r.level })})`);
  const technical = [
    ...e.rules.map((r) => text("grd.tech.rule", { rule: r.rule, level: r.level })),
    text("grd.tech.layer", { layer: e.layer }),
    text("grd.tech.actor", { actor: agentName(e.actor, t) }),
    text("grd.tech.decision", { id: e.decisionId }),
  ];
  const title = titleOf(e, t);
  return {
    id: e.decisionId,
    group: e.detail === "rate-limited" ? "unverified" : "blocks",
    kind: e.kind,
    title,
    nothingHappened,
    attempted: attempted(e, t, lang),
    why,
    stoppedBy,
    technical,
    exception: exceptionOf(e, t),
    hint: hintOf(e, t),
    ctaDisabled: live ? null : "engine-down",
    a11yLabel: [title, formatClock(e.lastMs, lang), ...stoppedBy].join(", "),
  };
}

/** The rows, newest first by `lastMs`; an unavailable read keeps showing its last entries. */
export function blockRows(blocks: GuardBlocks, live: boolean, t: TFunction, lang: string): BlockRow[] {
  return [...blocks.entries]
    .sort((a, b) => b.lastMs - a.lastMs)
    .map((e) => rowOf(e, live, t, lang));
}

/** The denials newer than `seenAfterMs`. */
export function unseen(entries: readonly BlockEntryDto[], seenAfterMs: number): BlockEntryDto[] {
  return entries.filter((e) => e.kind === "denial" && e.lastMs > seenAfterMs);
}

/** A stable text for an operation, so two of them compare by content (refs in any order). */
export function operationKey(op: OperationDto): string {
  if (op.kind === "push" || op.kind === "ref-transaction") {
    const refs = op.refs.map((r) => `${r.change}:${r.name}`).sort();
    return JSON.stringify([op.kind, op.kind === "push" ? op.remote : null, refs]);
  }
  return JSON.stringify([op.kind]);
}

const ruleSet = (rules: readonly string[]) => JSON.stringify([...new Set(rules)].sort());

/** Whether the announced exception differs from the copied one (D12, M-01); without a copy it never does. */
export function pendingMismatch(pending: PendingExceptionDto | null, copied: CopiedException): boolean {
  if (pending === null || copied === null) return false;
  return operationKey(pending.operation) !== operationKey(copied.operation) || ruleSet(pending.rules) !== ruleSet(copied.rules);
}
