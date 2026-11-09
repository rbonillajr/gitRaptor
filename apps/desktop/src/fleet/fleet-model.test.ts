// DS-US-DSK-004 T004 (§ 9.2, § 9.4): every rule of the card model, in en and es.
import i18n from "i18next";
import { describe, expect, it } from "vitest";
import "@/i18n";
import en from "@/i18n/locales/en.json";
import es from "@/i18n/locales/es.json";
import { fleetCards, formatAge, knownConflicts, type CardModel } from "./fleet-model";
import { HOUR, MIN, NOW, counted, pair, ready, session, tienda, worktree } from "./fixtures";

const t = i18n.getFixedT("en");
const tEs = i18n.getFixedT("es");
const cards = (fleet = tienda(), now = NOW, activity = true) => fleetCards(fleet, now, activity, t);
const card = (list: CardModel[], folder: string) => {
  const c = list.find((x) => x.folder === folder);
  if (!c) throw new Error(`no card ${folder}`);
  return c;
};
const order = (list: CardModel[]) => list.map((c) => c.folder);

describe("order and groups (D4)", () => {
  it("one card per worktree: main first, then the agents", () => {
    const list = cards();
    expect(order(list)).toEqual(["tienda", "feat-login", "feat-pagos"]);
    expect(list.map((c) => c.group)).toEqual(["main", "active", "active"]);
    expect(card(list, "feat-pagos")).toMatchObject({
      symbol: "●",
      stateText: "active",
      branchText: "feat-pagos",
      filesText: "2 changed files",
      syncText: "3 ahead and 1 behind main",
      activityText: "2 min ago",
    });
    expect(card(list, "feat-pagos").agent).toMatchObject({ label: "Claude Code · feat-pagos", muted: false, shared: false });
  });

  it("every group in order: main, attention, active, inactive, ended, no agent", () => {
    const names = ["z-noagent", "y-ended", "x-inactive", "w-active", "v-attention"];
    const fleet = tienda({
      worktrees: [
        worktree("tienda", { main: true }),
        ...names.map((n) => worktree(n)),
      ],
      sessions: [
        session("s-active", "w-active"),
        session("s-inactive", "x-inactive", { state: "inactive" }),
        session("s-ended", "y-ended", { state: "ended", endedUtcMs: NOW - HOUR, endCause: "process-gone" }),
        session("s-att", "v-attention"),
      ],
      pairs: [pair("v-attention", "w-active")],
    });
    // w-active also has the conflict, so it joins the attention group; v-attention sorts first by folder.
    const list = cards(fleet);
    expect(list.map((c) => [c.folder, c.group])).toEqual([
      ["tienda", "main"],
      ["v-attention", "attention"],
      ["w-active", "attention"],
      ["x-inactive", "inactive"],
      ["y-ended", "ended"],
      ["z-noagent", "no-agent"],
    ]);
  });

  it("a predicted conflict puts feat-login before feat-pagos with its reason", () => {
    const fleet = tienda({
      worktrees: [worktree("tienda", { main: true }), worktree("feat-login"), worktree("feat-pagos"), worktree("aaa-other")],
      sessions: [session("claude-1", "feat-pagos"), session("claude-2", "feat-login"), session("claude-3", "aaa-other")],
      pairs: [pair("feat-login", "base")],
    });
    const list = cards(fleet);
    expect(order(list)).toEqual(["tienda", "feat-login", "aaa-other", "feat-pagos"]);
    expect(card(list, "feat-login")).toMatchObject({ group: "attention", attention: { kind: "conflict", text: "⚡ predicted conflict with main", conflictWith: ["base"] } });
    expect(card(list, "feat-pagos").attention).toBeNull();
  });

  it("a card only moves when its group changes", () => {
    const base = tienda();
    const before = order(cards(base));
    const busy = tienda({
      worktrees: base.repo.worktrees.map((w) =>
        w.folder === "feat-login" ? { ...w, status: ready({ divergence: counted(9, 9), counts: { staged: 5, unstaged: 0, untracked: 0 } }), lastActivityUtcMs: NOW } : w,
      ),
    });
    expect(order(cards(busy))).toEqual(before);
    const idle = tienda({ sessions: [session("claude-1", "feat-pagos", { state: "inactive" }), session("claude-2", "feat-login")] });
    expect(order(cards(idle))).toEqual(["tienda", "feat-login", "feat-pagos"]);
    const swapped = tienda({ sessions: [session("claude-1", "feat-pagos"), session("claude-2", "feat-login", { state: "inactive" })] });
    expect(order(cards(swapped))).toEqual(["tienda", "feat-pagos", "feat-login"]);
  });

  it("ties inside a group break by folder, then path, then key", () => {
    const fleet = tienda({
      worktrees: [
        worktree("same", { key: "k2", path: "/b/same" }),
        worktree("same", { key: "k1", path: "/b/same" }),
        worktree("same", { key: "k0", path: "/a/same" }),
      ],
      sessions: [],
    });
    expect(cards(fleet).map((c) => c.key)).toEqual(["k0", "k1", "k2"]);
  });

  it("the main worktree stays in the main group even with a conflict", () => {
    const fleet = tienda({
      worktrees: [worktree("tienda", { main: true }), worktree("feat-login")],
      sessions: [],
      pairs: [pair("tienda", "feat-login")],
    });
    const list = cards(fleet);
    expect(list[0]).toMatchObject({ folder: "tienda", group: "main", main: true });
    expect(list[0]?.attention?.text).toBe("⚡ predicted conflict with feat-login");
    expect(list[1]?.group).toBe("attention");
  });
});

describe("agent (D5, D6)", () => {
  it("names a non-Claude agent with its folder", () => {
    const fleet = tienda({ sessions: [session("claude-1", "feat-pagos", { agent: { kind: "other", name: "aider" } })] });
    expect(card(cards(fleet), "feat-pagos").agent.label).toBe("aider · feat-pagos");
  });

  it("two sessions in one worktree: +N, shared, the active and most recent first", () => {
    const fleet = tienda({
      sessions: [
        session("claude-1", "feat-pagos", { state: "inactive", stateSinceUtcMs: NOW - MIN }),
        session("claude-2", "feat-pagos", { agent: { kind: "other", name: "claude-2" } }),
        session("claude-3", "feat-pagos", { agent: { kind: "other", name: "claude-3" }, stateSinceUtcMs: NOW - 10 * MIN }),
      ],
    });
    const c = card(cards(fleet), "feat-pagos");
    expect(c.agent).toMatchObject({ label: "claude-2 · feat-pagos +2", shared: true, sessionId: "claude-2" });
    expect(c.group).toBe("active");
    expect(c.a11yLabel).toContain("shared");
  });

  it("session tie: the most recent change, then the id", () => {
    const fleet = tienda({
      sessions: [
        session("b", "feat-pagos", { agent: { kind: "other", name: "bee" } }),
        session("a", "feat-pagos", { agent: { kind: "other", name: "ay" } }),
      ],
    });
    expect(card(cards(fleet), "feat-pagos").agent.sessionId).toBe("a");
  });

  it("an ended session shows for 24 hours, then the last agent", () => {
    const ended = (ago: number) => session("claude-1", "feat-pagos", { state: "ended", endedUtcMs: NOW - ago, endCause: "process-gone" });
    const inside = card(cards(tienda({ sessions: [ended(2 * HOUR)] })), "feat-pagos");
    expect(inside).toMatchObject({ group: "ended", symbol: "○", stateText: "ended 2 h ago", agentState: "ended" });
    expect(inside.agent).toMatchObject({ muted: true, label: "Claude Code · feat-pagos", sessionId: null });

    const edge = card(cards(tienda({ sessions: [ended(24 * HOUR - 1)] })), "feat-pagos");
    expect(edge.group).toBe("ended");

    const outside = card(cards(tienda({ sessions: [ended(24 * HOUR)] })), "feat-pagos");
    expect(outside).toMatchObject({ group: "no-agent", symbol: "○", agentState: "none", stateText: "last agent: Claude Code (ended 1 d ago)" });
    expect(outside.agent.label).toBe("No agent · feat-pagos");
  });

  it("only the latest ended session of a worktree counts", () => {
    const fleet = tienda({
      sessions: [
        session("old", "feat-pagos", { state: "ended", endedUtcMs: NOW - 20 * HOUR, endCause: "process-gone" }),
        session("new", "feat-pagos", { state: "ended", endedUtcMs: NOW - 5 * MIN, endCause: "process-gone", agent: { kind: "other", name: "aider" } }),
      ],
    });
    expect(card(cards(fleet), "feat-pagos")).toMatchObject({ stateText: "ended 5 min ago" });
    expect(card(cards(fleet), "feat-pagos").agent.label).toBe("aider · feat-pagos");
  });

  it("a present session beats an ended one", () => {
    const fleet = tienda({
      sessions: [session("now", "feat-pagos"), session("then", "feat-pagos", { state: "ended", endedUtcMs: NOW - MIN, endCause: "process-gone" })],
    });
    expect(card(cards(fleet), "feat-pagos").group).toBe("active");
  });

  it("an ended session without an end time counts the window from its last change", () => {
    const gap = session("g", "feat-pagos", { state: "ended", endCause: "ended-during-gap", stateSinceUtcMs: NOW - 3 * HOUR });
    expect(card(cards(tienda({ sessions: [gap] })), "feat-pagos").stateText).toBe("ended unobserved; detected 3 h ago");
    const other = session("o", "feat-pagos", { state: "ended", endCause: "worktree-removed", stateSinceUtcMs: NOW - 3 * HOUR });
    expect(card(cards(tienda({ sessions: [other] })), "feat-pagos").stateText).toBe("end time not available");
    const stale = session("s", "feat-pagos", { state: "ended", endCause: "ended-during-gap", stateSinceUtcMs: NOW - 30 * HOUR });
    expect(card(cards(tienda({ sessions: [stale] })), "feat-pagos").group).toBe("no-agent");
  });

  it("no session and known detection says no agent", () => {
    const c = card(cards(tienda({ sessions: [] })), "feat-pagos");
    expect(c).toMatchObject({ group: "no-agent", symbol: "○", agentState: "none", stateText: "" });
    expect(c.agent.label).toBe("No agent · feat-pagos");
  });

  it.each([null, false] as const)("unknown detection (%s) never says no agent", (detection) => {
    const list = cards(tienda({ sessions: [], detection }));
    for (const c of list) {
      expect(c).toMatchObject({ symbol: "", agentState: "unavailable", group: c.main ? "main" : "no-agent" });
      expect(c.agent.label).toBe("agent not available");
      expect(c.a11yLabel).not.toContain("No agent");
    }
  });

  it("unknown detection still shows the sessions that are present", () => {
    const c = card(cards(tienda({ detection: false })), "feat-pagos");
    expect(c).toMatchObject({ group: "active", agentState: "active" });
  });

  it("the Spanish texts follow", () => {
    const c = fleetCards(tienda({ sessions: [session("claude-1", "feat-pagos")] }), NOW, true, tEs).find((x) => x.folder === "feat-pagos");
    expect(c?.stateText).toBe("activo");
    expect(c?.syncText).toBe("3 por delante y 1 por detrás de main");
    expect(c?.filesText).toBe("2 archivos cambiados");
    expect(c?.activityText).toBe("hace 2 min");
  });
});

describe("branch, files and ahead/behind", () => {
  const one = (over: Parameters<typeof worktree>[1]) => card(cards(tienda({ worktrees: [worktree("w", over)], sessions: [] })), "w");

  it("shows the branch name, or an unborn branch", () => {
    expect(one({ status: ready({ head: { kind: "branch", name: "feat/x" } }) }).branchText).toBe("feat/x");
    expect(one({ status: ready({ head: { kind: "unborn", name: "main" } }) }).branchText).toBe("main (no commits)");
  });

  it("a detached worktree shows 7 digits or no hash", () => {
    const sha = "0123456789abcdef0123456789abcdef01234567";
    expect(one({ status: ready({ head: { kind: "detached" } }), detachedAt: sha })).toMatchObject({ branchText: "0123456 (no branch)", branchCommit: "0123456" });
    const sha256 = "a".repeat(64);
    expect(one({ status: ready({ head: { kind: "detached" } }), detachedAt: sha256 }).branchText).toBe("aaaaaaa (no branch)");
    expect(one({ status: ready({ head: { kind: "detached" } }), detachedAt: null })).toMatchObject({ branchText: "(no branch)", branchCommit: null });
  });

  it("counts staged, unstaged and untracked as changed files; none says no changes", () => {
    expect(one({ status: ready({ counts: { staged: 1, unstaged: 2, untracked: 3 } }) })).toMatchObject({ filesCount: 6, filesText: "6 changed files" });
    expect(one({ status: ready({ counts: { staged: 0, unstaged: 1, untracked: 0 } }) }).filesText).toBe("1 changed file");
    expect(one({ status: ready() })).toMatchObject({ filesCount: 0, filesText: "no changes" });
  });

  it("shows ↑↓ with a plus when a count is capped, zero when it is really zero", () => {
    expect(one({ status: ready({ divergence: counted(0, 0) }) }).syncText).toBe("0 ahead and 0 behind main");
    expect(one({ status: ready({ divergence: counted(10_000, 2, [false, true]) }) }).syncText).toBe("10000+ ahead and 2 behind main");
    expect(one({ status: ready({ divergence: counted(1, 10_000, [true, false]) }) }).syncText).toBe("1 ahead and 10000+ behind main");
  });

  it.each([
    ["base-missing", "not computable"],
    ["no-base", "no base branch"],
    ["no-commits", "no commits"],
    ["unreadable", "unreadable now"],
  ] as const)("a %s divergence says %s, never zero or a dash", (state, text) => {
    const c = one({ status: ready({ divergence: { state } }) });
    expect(c.syncText).toBe(text);
    expect(c.syncText).not.toMatch(/^[0–-]/);
  });

  it("a missing base says not computable, never zero", () => {
    const fleet = tienda({ worktrees: [worktree("w", { status: ready({ divergence: { state: "base-missing" } }) })], sessions: [] });
    expect(cards(fleet)[0]?.syncText).toBe("not computable");
  });
});

describe("activity", () => {
  const act = (ms: number | null, activity = true) =>
    card(cards(tienda({ worktrees: [worktree("w", { lastActivityUtcMs: ms })], sessions: [] }), NOW, activity), "w").activityText;

  it("uses the largest whole unit", () => {
    expect(act(NOW - 5_000)).toBe("5 s ago");
    expect(act(NOW - 59_999)).toBe("59 s ago");
    expect(act(NOW - 60_000)).toBe("1 min ago");
    expect(act(NOW - 59 * MIN - 59_000)).toBe("59 min ago");
    expect(act(NOW - 90 * MIN)).toBe("1 h ago");
    expect(act(NOW - 23 * HOUR - 59 * MIN)).toBe("23 h ago");
    expect(act(NOW - 49 * HOUR)).toBe("2 d ago");
  });

  it("a future timestamp never goes negative", () => {
    expect(act(NOW + 5_000)).toBe("0 s ago");
    expect(formatAge(-1, t)).toBe("0 s");
  });

  it("says a dash without data or without session activity", () => {
    expect(act(null)).toBe("–");
    expect(act(NOW - MIN, false)).toBe("–");
  });
});

describe("attention (known conflicts)", () => {
  it("only current and recalculating conflicts count", () => {
    const pairs = [
      pair("a", "b", { state: "current" }),
      pair("a", "c", { state: "recalculating" }),
      pair("a", "d", { state: "pending" }),
      pair("a", "e", { state: "calculating" }),
      pair("a", "f", { state: "not-computable" }),
      pair("a", "g", { level: "overlap" }),
      pair("a", "h", { level: "none" }),
      pair("a", "i", { level: null }),
      pair("x", "y"),
    ];
    expect(knownConflicts(pairs, "key-a")).toEqual(["key-b", "key-c"]);
    expect(knownConflicts(pairs, "key-b")).toEqual(["key-a"]);
    expect(knownConflicts(pairs, "key-zzz")).toEqual([]);
  });

  it("the other side may be the base, and a pair is listed once", () => {
    expect(knownConflicts([pair("a", "base"), pair("a", "base", { pairId: "dup" })], "key-a")).toEqual(["base"]);
  });

  it("names the other worktree, the base, or how many", () => {
    const worktrees = [worktree("tienda", { main: true }), worktree("feat-login"), worktree("feat-pagos"), worktree("feat-x")];
    const withPairs = (pairs: ReturnType<typeof pair>[]) => cards(tienda({ worktrees, sessions: [], pairs }));
    expect(card(withPairs([pair("feat-login", "feat-pagos")]), "feat-login").attention?.text).toBe("⚡ predicted conflict with feat-pagos");
    expect(card(withPairs([pair("feat-login", "feat-pagos")]), "feat-pagos").attention?.text).toBe("⚡ predicted conflict with feat-login");
    expect(card(withPairs([pair("feat-login", "base")]), "feat-login").attention?.text).toBe("⚡ predicted conflict with main");
    const many = withPairs([pair("feat-login", "feat-pagos"), pair("feat-login", "feat-x")]);
    expect(card(many, "feat-login").attention?.text).toBe("⚡ predicted conflicts with 2 worktrees");
  });

  it("a conflict does not come from a worktree with no known conflict", () => {
    const list = cards(tienda({ pairs: [pair("feat-login", "feat-pagos", { state: "pending" })] }));
    expect(list.every((c) => c.attention === null && c.group !== "attention")).toBe(true);
  });

  it("the attention row is the only attention kind", () => {
    const list = cards(tienda({ pairs: [pair("feat-login", "feat-pagos")] }));
    expect(new Set(list.flatMap((c) => (c.attention ? [c.attention.kind] : [])))).toEqual(new Set(["conflict"]));
  });
});

describe("unavailable worktree", () => {
  it.each([
    ["missing", "unavailable: folder missing"],
    ["untrusted", "unavailable: Git does not trust it"],
    ["unreadable", "unavailable: unreadable now"],
  ] as const)("a %s worktree shows why and no metrics", (reason, text) => {
    const fleet = tienda({ worktrees: [worktree("gone", { status: { state: "unavailable", reason } })], sessions: [] });
    const c = cards(fleet)[0];
    expect(c).toMatchObject({
      symbol: "⚠",
      unavailableText: text,
      branchText: null,
      filesText: null,
      filesCount: null,
      syncText: null,
      activityText: null,
    });
    expect(c?.a11yLabel).toContain(text);
  });
});

describe("accessible name and catalogs", () => {
  it("is the visible parts joined with a comma, without the bolt glyph", () => {
    const fleet = tienda({
      worktrees: [worktree("tienda", { main: true, status: ready() }), worktree("feat-pagos", { status: ready({ head: { kind: "branch", name: "feat-pagos" }, counts: { staged: 1, unstaged: 1, untracked: 0 }, divergence: counted(3, 1) }) })],
      sessions: [session("claude-1", "feat-pagos")],
      pairs: [pair("feat-pagos", "base")],
    });
    expect(card(cards(fleet), "feat-pagos").a11yLabel).toBe(
      "Claude Code · feat-pagos, active, feat-pagos, 2 changed files, 3 ahead and 1 behind main, 2 min ago, predicted conflict with main",
    );
    expect(card(cards(fleet), "tienda").a11yLabel).toBe("main worktree, No agent · tienda, main, no changes, 0 ahead and 0 behind main, 2 min ago");
  });

  it("every card has a symbol or a state text, and a name", () => {
    const fleet = tienda({ sessions: [], detection: false });
    for (const c of cards(fleet)) {
      expect(c.symbol !== "" || c.agent.label !== "").toBe(true);
      expect(c.agent.label).not.toBe("");
    }
  });

  it("no card ever says human", () => {
    const texts = (node: unknown): string[] =>
      node !== null && typeof node === "object" ? Object.values(node).flatMap(texts) : [String(node)];
    for (const catalog of [en.fleet, es.fleet, en.repos, es.repos]) {
      for (const text of texts(catalog)) expect(text.toLowerCase()).not.toMatch(/human/);
    }
  });
});
