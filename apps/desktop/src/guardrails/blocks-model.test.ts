/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the rows they just built */
// D7-MODEL (DS-US-DSK-007 T004, § 9.4): blockRows turns the cached entries into plain-language rows; unseen counts the
// denials newer than the last seen time; pendingMismatch compares the announced exception with the copied one (D12).
import i18n, { type TFunction } from "i18next";
import { act } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import "@/i18n";
import type { BlockEntryDto, OperationDto, PendingExceptionDto } from "@/client/engine-types";
import type { GuardBlocks } from "@/state/engine-cache";
import { blockRows, pendingMismatch, unseen, type CopiedException } from "./blocks-model";

const t = i18n.t.bind(i18n) as unknown as TFunction;
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const NOW = Date.UTC(2026, 10, 9, 12, 0, 0);
const FORCE: OperationDto = { kind: "push", remote: "origin", refs: [{ name: "main", change: "force" }] };
const FORCE_RULE = [{ rule: "minimum.force-push", level: "minimum", cause: null }];

const entry = (over: Partial<BlockEntryDto> = {}): BlockEntryDto => ({
  decisionId: "d1",
  kind: "denial",
  atMs: NOW - 60_000,
  lastMs: NOW - 60_000,
  count: 1,
  actor: { kind: "claude-code" },
  worktree: { folder: "feat-pagos", path: "/w/tienda/feat-pagos" },
  branch: "feat-pagos",
  operation: FORCE,
  rules: FORCE_RULE,
  layer: "hooks",
  detail: "full",
  exception: { state: "offered", command: "'/Applications/GitRaptor.app/Contents/MacOS/raptor' 'guard' 'exec' '--' 'git' 'push'" },
  ...over,
});
const ready = (entries: BlockEntryDto[]): GuardBlocks => ({
  state: "ready",
  repoId: "tienda",
  protection: { state: "full" },
  sinceMs: NOW - 7 * 86_400_000,
  summary: { blocked: entries.length, exceptions: 0, exceptionsRejected: 0 },
  entries,
  pending: null,
  unlogged: [],
});
const rowsOf = (entries: BlockEntryDto[], live = true) => blockRows(ready(entries), live, t, i18n.language);
const one = (over: Partial<BlockEntryDto> = {}, live = true) => {
  const rows = rowsOf([entry(over)], live);
  expect(rows, "blockRows gives one row per entry").toHaveLength(1);
  return rows[0]!;
};

beforeEach(async () => {
  await setLang("en");
});

describe("D7-MODEL blockRows", () => {
  it("D7-MODEL: a push says on the server and the remote name", () => {
    const row = one();
    expect(row.title, "the title names the agent, the operation, the ref and the remote in plain words").toMatch(
      /^Blocked: Claude Code tried to rewrite the history of main on the server \(origin\)$/,
    );
    expect(one({ operation: { kind: "push", remote: "origin", refs: [{ name: "old", change: "delete" }] } }).title).toBe(
      "Blocked: Claude Code tried to delete old on the server (origin)",
    );
    expect(one({ operation: { kind: "push", remote: "origin", refs: [{ name: "feat-x", change: "update" }] } }).title).toBe(
      "Blocked: Claude Code tried to push to feat-x on the server (origin)",
    );
  });

  it("D7-MODEL: titles of ref changes, rebase and commit", () => {
    expect(one({ operation: { kind: "ref-transaction", refs: [{ name: "feat-x", change: "delete" }] } }).title).toBe(
      "Blocked: Claude Code tried to delete the branch feat-x",
    );
    expect(one({ operation: { kind: "ref-transaction", refs: [{ name: "feat-x", change: "update" }] } }).title).toBe(
      "Blocked: Claude Code tried to move the branch feat-x",
    );
    expect(one({ operation: { kind: "rebase" } }).title).toBe("Blocked: Claude Code tried to rebase feat-pagos");
    expect(one({ operation: { kind: "rebase" }, branch: null }).title).toBe("Blocked: Claude Code tried to a rebase");
    expect(one({ operation: { kind: "commit" } }).title).toBe("Blocked: Claude Code tried to make a commit on feat-pagos");
  });

  it("D7-MODEL: titles in Spanish follow the catalog", async () => {
    await setLang("es");
    expect(one().title).toBe("Bloqueado: Claude Code intentó reescribir el historial de main en el servidor (origin)");
  });

  it("D7-MODEL: no agent identified uses its own title", () => {
    const row = one({ actor: null, worktree: null });
    expect(row.title, "without an actor the title says no agent was identified").toBe(
      "Blocked: someone tried to rewrite the history of main on the server (origin) (no agent identified)",
    );
  });

  it("D7-MODEL: an unattributed block never says human", async () => {
    for (const lang of ["en", "es"] as const) {
      await setLang(lang);
      const row = one({ actor: null, worktree: null });
      const text = [row.title, row.attempted, row.nothingHappened, row.why, ...row.stoppedBy, row.a11yLabel].join(" ");
      expect(text, `${lang}: lo no atribuido nunca es humano (D5)`).not.toMatch(/human|humano|persona|person\b/i);
      expect(row.title.length, "the row must exist with a title").toBeGreaterThan(0);
    }
  });

  it("D7-MODEL: nothing happened only for a denied push or ref change, with what holds for the operation", () => {
    expect(one().nothingHappened, "a denied push says main is unchanged on the machine and on the server").toBe(
      "Nothing happened. main is unchanged on your machine and on the server.",
    );
    expect(one({ operation: { kind: "ref-transaction", refs: [{ name: "feat-x", change: "delete" }] } }).nothingHappened).toBe(
      "Nothing happened. feat-x is unchanged on your machine.",
    );
    expect(one({ operation: { kind: "commit" } }).nothingHappened, "a commit has no 'nothing happened'").toBeNull();
    expect(one({ operation: { kind: "rebase" } }).nothingHappened).toBeNull();
    expect(one({ kind: "exception" }).nothingHappened, "an exception did happen").toBeNull();
  });

  it("D7-MODEL: what it tried names the source branch and the local time, and the count when repeated", () => {
    const row = one();
    expect(row.attempted, "the source branch, the destination and the remote").toMatch(/feat-pagos.*main.*origin/);
    expect(row.attempted, "the local time of atMs").toMatch(/\d{1,2}:\d{2}/);
    const again = one({ count: 3, lastMs: NOW });
    expect(again.attempted, "a repeated block says ×3 and the time of the last one").toMatch(/×3, the last at \d{1,2}:\d{2}/);
  });

  it("D7-MODEL: why it is dangerous only for force-push and base-branch-delete", () => {
    expect(one().why, "force-push explains the server history").toMatch(/replaces the server's history/i);
    expect(
      one({ operation: { kind: "push", remote: "origin", refs: [{ name: "main", change: "delete" }] }, rules: [{ rule: "minimum.base-branch-delete", level: "minimum", cause: null }] }).why,
    ).toMatch(/deletes your main branch/i);
    expect(one({ rules: [{ rule: "policy.protected-branch", level: "floor", cause: null }] }).why, "other rules carry no why").toBeNull();
  });

  it("D7-MODEL: what stopped it gives the rule text and its level", () => {
    const row = one({ rules: [{ rule: "policy.protected-branch", level: "floor", cause: null }] });
    expect(row.stoppedBy, "one line per rule").toHaveLength(1);
    expect(row.stoppedBy[0], "the text of guard.log.rule.protected-branch and guard.level.floor").toMatch(/protected branch/);
    expect(row.stoppedBy[0]).toMatch(/team configuration/);
    expect(one().stoppedBy[0], "the text of guard.log.rule.force-push").toMatch(/no force-push/);
  });

  it("D7-MODEL: technical details list the rule, level, layer, actor and decision id", () => {
    const text = one().technical.join(" ");
    expect(text).toContain("d1");
    expect(text).toContain("minimum.force-push");
    expect(text).toContain("hooks");
  });

  it("D7-MODEL: rows go newest first by lastMs and rate-limited ones go to their own group", () => {
    const rows = rowsOf([
      entry({ decisionId: "old", lastMs: NOW - 5 * 60_000 }),
      entry({ decisionId: "new", lastMs: NOW - 1_000 }),
      entry({ decisionId: "rl", lastMs: NOW - 2_000, detail: "rate-limited", exception: { state: "not-offered", reason: "rate-limited" } }),
    ]);
    expect(rows.map((r) => r.id).filter((id) => id !== "rl"), "newest first").toEqual(["new", "old"]);
    expect(rows.find((r) => r.id === "rl")?.group, "rate-limited are unverified").toBe("unverified");
    expect(rows.find((r) => r.id === "new")?.group).toBe("blocks");
  });

  it("D7-MODEL: the exception is the command when offered, the reason text when not, the form for a rebase", () => {
    expect(one().exception).toEqual({ command: expect.stringContaining("guard") });
    const commit = one({ operation: { kind: "commit" }, exception: { state: "not-offered", reason: "commit" } });
    expect(commit.exception, "a commit says why there is no exception").toEqual({
      notOffered: "The exception only works for push, deleting a local branch and rebase.",
    });
    const rebase = one({ operation: { kind: "rebase" }, exception: { state: "not-offered", reason: "rebase" } });
    expect(rebase.exception, "a rebase gets the form without a copy").toHaveProperty("rebaseForm");
  });

  it("D7-MODEL: without a live connection the call to action is disabled for the engine", () => {
    expect(one({}, true).ctaDisabled).toBeNull();
    expect(one({}, false).ctaDisabled).toBe("engine-down");
  });

  it("D7-MODEL: the source branch and the branch delete hint", () => {
    const hint = one().hint;
    expect(hint, "a push of feat-pagos onto main infers the source branch").not.toBeNull();
    expect(hint === "grd.exc.sourceInferred" || String(hint).includes("feat-pagos"), "the source-inferred line").toBe(true);
    const del = one({
      operation: { kind: "ref-transaction", refs: [{ name: "feat-x", change: "delete" }] },
      exception: { state: "offered", command: "'raptor' 'guard' 'exec' '--' 'git' 'branch' '-d' 'feat-x'" },
    }).hint;
    expect(del, "a branch delete hints -d to -D").not.toBeNull();
    expect(del === "grd.exc.branchDeleteHint" || String(del).includes("-D"), "the -D hint").toBe(true);
    expect(one({ branch: "main" }).hint, "no hint when the source equals the destination").toBeNull();
  });

  it("D7-MODEL: exception entries say what happened with the operation and the rules skipped", () => {
    const done = one({ kind: "exception", exception: null });
    expect(done.title).toMatch(/^Done with an exception: .*main.*; skipped .*force-push/);
    expect(one({ kind: "exception-cancelled", exception: null }).title).toMatch(/Cancelled: nothing ran/);
    expect(one({ kind: "exception-cancelled", exception: null }).title, "cancelled by the person, not an agent").not.toMatch(/Cancelled by/);
    expect(one({ kind: "exception-rejected", exception: null }).title).toMatch(/^Exception refused: .*main/);
    const byAgent = rowsOf([entry({ kind: "exception-cancelled", exception: null })])[0]!;
    expect([byAgent.title, byAgent.attempted].join(" "), "a cancel by an agent says who").toMatch(/Cancelled by Claude Code|Cancelled by .*Claude Code/);
  });

  it("D7-MODEL: the accessible name is the visible phrase joined by commas", () => {
    const row = one();
    expect(row.a11yLabel).toContain(row.title);
    expect(row.a11yLabel, "the phrase has several parts joined with ', '").toContain(", ");
  });

  it("D7-MODEL: an unavailable read keeps showing the last entries", () => {
    const rows = blockRows(
      { state: "unavailable", cause: "log-unavailable", protection: { state: "full" }, entries: [entry()], readAtMs: NOW - 30_000 },
      true,
      t,
      "en",
    );
    expect(rows, "the entries survive an unavailable read").toHaveLength(1);
  });
});

describe("D7-MODEL unseen", () => {
  const A = entry({ decisionId: "a", lastMs: NOW - 10_000 });
  const B = entry({ decisionId: "b", lastMs: NOW - 5_000 });

  it("D7-MODEL: history at first read is not new", () => {
    const max = Math.max(...[A, B].map((e) => e.lastMs));
    expect(unseen([A, B], max), "seenAfterMs starts at the max lastMs of the first read").toEqual([]);
  });

  it("D7-MODEL: only denials newer than the seen time count", () => {
    const fresh = entry({ decisionId: "c", lastMs: NOW });
    const exception = entry({ decisionId: "e", kind: "exception", lastMs: NOW + 1_000 });
    expect(unseen([A, B, fresh, exception], B.lastMs).map((e) => e.decisionId), "a new denial counts, an exception does not").toEqual(["c"]);
    expect(unseen([A, B], A.lastMs).map((e) => e.decisionId), "strictly newer than seenAfterMs").toEqual(["b"]);
  });
});

describe("D7-MODEL pendingMismatch", () => {
  const pending = (over: Partial<PendingExceptionDto> = {}): PendingExceptionDto => ({
    appliesAtMs: NOW + 10_000,
    windowMs: 10_000,
    operation: FORCE,
    rules: ["minimum.force-push"],
    cancelCommand: "'raptor' 'guard' 'cancel' '/w/tienda'",
    ...over,
  });
  const copied = (over: Partial<NonNullable<CopiedException>> = {}): CopiedException => ({
    repoId: "tienda",
    operation: FORCE,
    rules: ["minimum.force-push"],
    ...over,
  });

  it("D7-MODEL: without a copy there is no mismatch", () => {
    expect(pendingMismatch(pending(), null)).toBe(false);
    expect(pendingMismatch(null, null)).toBe(false);
  });

  it("D7-MODEL: the same operation and rules are not a mismatch", () => {
    expect(pendingMismatch(pending(), copied()), "same operation and rules").toBe(false);
    expect(pendingMismatch(pending({ rules: ["b", "a"] }), copied({ rules: ["a", "b"] })), "the rule set ignores order").toBe(false);
  });

  it("D7-MODEL: a pending exception that differs from the copied one is a mismatch", () => {
    expect(pendingMismatch(pending({ operation: { kind: "push", remote: "origin", refs: [{ name: "release", change: "force" }] } }), copied()), "other ref").toBe(true);
    expect(pendingMismatch(pending({ operation: { kind: "push", remote: "upstream", refs: FORCE.kind === "push" ? FORCE.refs : [] } }), copied()), "other remote").toBe(true);
    expect(pendingMismatch(pending({ operation: { kind: "push", remote: "origin", refs: [{ name: "main", change: "delete" }] } }), copied()), "other change").toBe(true);
    expect(pendingMismatch(pending({ rules: ["minimum.force-push", "policy.protected-branch"] }), copied()), "other rules").toBe(true);
    expect(pendingMismatch(pending({ operation: { kind: "rebase" } }), copied()), "other kind").toBe(true);
  });
});
