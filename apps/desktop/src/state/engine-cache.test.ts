// D3-CACHE (DS-US-DSK-003 T006): applyEngineBatch leaves the query cache and the engine store as the Dev Spec says for
// every variant of the golden fixture (`tests/fixtures/engine-messages.json`, serialized from `wire.rs`), never mutates
// the previous object, and writes only with `setQueryData` (no network queryFn). D4 (DS-US-DSK-004 T003) adds the six
// repo variants: only `applyEngineBatch` writes `engineKeys.repo`, and `useFleet` reads it without a network queryFn.
import { QueryClient } from "@tanstack/react-query";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineMessage, GlobalDto, RepoFleet, SessionDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { session as sessionOf, tienda } from "@/fleet/fixtures";
import fixture from "../../tests/fixtures/engine-messages.json";
import { applyEngineBatch } from "./engine-cache";
import { useEngineStore } from "./engine-store";
import { useUiStore } from "./ui-store";

type Variant = EngineMessage["type"];
const F = fixture as unknown as Record<Variant, EngineMessage>;
const VARIANTS = [
  "conn",
  "session",
  "global",
  "engineState",
  "repoObserved",
  "repoRetired",
  "repoTier",
  "repoAttention",
  "discovered",
  "engineOnly",
  "repo",
  "worktrees",
  "sessions",
  "repoSession",
  "prediction",
  "repoClosed",
] as const satisfies readonly Variant[];

/** The message of a variant, typed. */
function msg<T extends Variant>(type: T): Extract<EngineMessage, { type: T }> {
  return F[type] as Extract<EngineMessage, { type: T }>;
}
/** A message without its `type` tag: what the DTOs look like in the cache and the store. */
function untagged<T extends { type: string }>(message: T): Omit<T, "type"> {
  return Object.fromEntries(Object.entries(message).filter(([key]) => key !== "type")) as Omit<T, "type">;
}

const INITIAL = { conn: { kind: "connecting" }, staleSinceMs: null, resync: null, session: null } as const;
let client: QueryClient;

beforeEach(() => {
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
  useEngineStore.setState(INITIAL);
});

const apply = (...batch: EngineMessage[]) => applyEngineBatch(client, batch);
const global = () => client.getQueryData<GlobalDto & { restricted?: boolean }>(engineKeys.global);
const store = () => useEngineStore.getState();

describe("D3-CACHE golden fixture", () => {
  it("D3-CACHE: the fixture has one example of every EngineMessage variant, tagged with its type", () => {
    expect(Object.keys(F).sort()).toEqual([...VARIANTS].sort());
    for (const key of VARIANTS) expect(F[key].type, `fixture[${key}].type`).toBe(key);
  });

  it("D3-CACHE: conn sets the connection, the stale time and the resync reason in the store only", () => {
    const m = msg("conn");
    apply(m);
    expect(store().conn).toEqual(m.state);
    expect(store().staleSinceMs).toBe(m.staleSinceMs);
    expect(store().resync).toBe(m.resync);
    expect(client.getQueryCache().findAll(), "conn must not write the cache").toHaveLength(0);
  });

  it("D3-CACHE: session sets the session in the store and does not clear the cache", () => {
    apply(msg("global"));
    const before = global();
    apply(msg("session"));
    expect(store().session).toEqual(untagged(msg("session")));
    expect(global(), "a new session keeps the cache until the next snapshot").toEqual(before);
  });

  it("D3-CACHE: global replaces the global entry with its DTO", () => {
    apply(msg("global"));
    expect(global()).toEqual(untagged(msg("global")));
    expect(store().conn, "a snapshot does not touch the store").toEqual(INITIAL.conn);
    const replacement = { ...msg("global"), repos: [] };
    apply(replacement);
    expect(global()?.repos, "the next snapshot replaces, never merges").toEqual([]);
  });

  it("D3-CACHE: engineState updates the engine of the global entry", () => {
    apply(msg("global"), msg("engineState"));
    expect(global()?.engine).toEqual(msg("engineState").engine);
    expect(global()?.repos).toEqual(msg("global").repos);
  });

  it("D3-CACHE: repoObserved upserts a repo by id", () => {
    apply(msg("global"), msg("repoObserved"));
    const ids = global()?.repos.map((r) => r.repoId);
    expect(ids).toEqual(["repo-alpha", "repo-beta", "repo-gamma"]);
    const [alpha] = msg("global").repos;
    if (!alpha) throw new Error("the fixture has no first repo");
    const renamed = { ...alpha, name: "alpha-renamed" };
    apply({ type: "repoObserved", repo: renamed });
    expect(global()?.repos).toHaveLength(3);
    expect(global()?.repos[0]?.name).toBe("alpha-renamed");
  });

  it("D3-CACHE: repoRetired removes the repo", () => {
    apply(msg("global"), msg("repoRetired"));
    expect(global()?.repos.map((r) => r.repoId)).toEqual(["repo-alpha"]);
  });

  it("D3-CACHE: repoTier sets the tier of that repo only", () => {
    apply(msg("global"), msg("repoTier"));
    expect(global()?.repos.map((r) => r.tier)).toEqual(["dormant", null]);
  });

  it("D3-CACHE: repoAttention sets the attention of that repo only", () => {
    apply(msg("global"), msg("repoAttention"));
    expect(global()?.repos[0]?.attention).toEqual(msg("repoAttention").attention);
    expect(global()?.repos[1]?.attention).toEqual(msg("global").repos[1]?.attention);
  });

  it("D3-CACHE: discovered replaces the candidates, or adds when replace is false", () => {
    const first = msg("discovered");
    apply(first);
    expect(client.getQueryData(engineKeys.discovered)).toEqual(first.candidates);
    const more = [{ name: "epsilon", path: "/Users/me/code/epsilon" }];
    apply({ type: "discovered", candidates: more, replace: false });
    expect(client.getQueryData(engineKeys.discovered)).toEqual([...first.candidates, ...more]);
    apply(first);
    expect(client.getQueryData(engineKeys.discovered)).toEqual(first.candidates);
  });

  it("D3-CACHE: engineOnly writes the global entry with the engine, no repos and restricted", () => {
    apply(msg("global"), msg("engineOnly"));
    expect(global()).toMatchObject({ engine: msg("engineOnly").engine, repos: [], restricted: true });
  });

  it.each(["engineState", "repoObserved", "repoRetired", "repoTier", "repoAttention", "engineOnly"] as const)(
    "D3-CACHE: %s does not mutate the previous object",
    (variant) => {
      apply(msg("global"));
      const previous = global();
      const copy = structuredClone(previous);
      apply(msg(variant));
      expect(previous, "the old object is untouched").toEqual(copy);
      expect(global(), "a new object is stored").not.toBe(previous);
    },
  );
});

describe("D3-CACHE keys and writers", () => {
  it("D3-CACHE: the keys are the Dev Spec's and only applyEngineBatch writes engineKeys.repo, which useFleet reads without a queryFn", () => {
    expect(engineKeys.global).toEqual(["engine", "global"]);
    expect(engineKeys.discovered).toEqual(["engine", "discovered"]);
    expect(engineKeys.repo("r1")).toEqual(["engine", "repo", "r1"]);
    apply(
      msg("global"),
      msg("engineState"),
      msg("repoObserved"),
      msg("repoRetired"),
      msg("repoTier"),
      msg("repoAttention"),
      msg("discovered"),
      msg("engineOnly"),
      msg("conn"),
      msg("session"),
    );
    expect(client.getQueryCache().findAll({ queryKey: ["engine", "repo"] }), "the global messages write no repo entry").toHaveLength(0);
    const queries = client.getQueryCache().findAll();
    expect(queries.length, "the batch writes the cache").toBeGreaterThan(0);
    for (const q of queries) expect(q.options.queryFn, `${q.queryHash} must have no network queryFn`).toBeUndefined();

    const root = fileURLToPath(new URL("..", import.meta.url));
    const sources = (readdirSync(root, { recursive: true, encoding: "utf8" }) as string[])
      .filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f) && !f.endsWith("client/queries.ts"))
      .map((f) => f.replaceAll("\\", "/"))
      .filter((f) => readFileSync(`${root}/${f}`, "utf8").includes("engineKeys.repo"));
    const writers = sources.filter((f) => /setQueryData|setQueriesData|removeQueries|fetchQuery|prefetchQuery|invalidateQueries|resetQueries/.test(readFileSync(`${root}/${f}`, "utf8")));
    expect(writers, "D4: only applyEngineBatch writes engineKeys.repo").toEqual(["state/engine-cache.ts"]);
    expect(sources, "D4: useFleet reads engineKeys.repo").toContain("fleet/useFleet.ts");
    const useFleet = readFileSync(`${root}/fleet/useFleet.ts`, "utf8");
    expect(useFleet, "D4: useFleet reads the cache with skipToken, never a network queryFn").toContain("skipToken");
    expect(useFleet, "D4: useFleet has no queryFn of its own").not.toMatch(/queryFn:\s*(?!skipToken)/);
  });
});

// ---------------------------------------------------------------- DS-US-DSK-004 T003

const fleetOf = (id = "tienda") => client.getQueryData<RepoFleet>(engineKeys.repo(id));
const base = () => tienda().repo;
const repoMsg = (id = "tienda"): EngineMessage => ({ type: "repo", fleet: { ...base(), repoId: id } });
const perf = { backendNs: 1_000_000 };
const sessionsMsg = (sessions: SessionDto[], detectionAvailable = true, repoId = "tienda"): EngineMessage => ({
  type: "sessions",
  repoId,
  detectionAvailable,
  sessions,
});
const repoSessionMsg = (session: SessionDto, repoId = "tienda"): EngineMessage => ({ type: "repoSession", repoId, session, perf });

beforeEach(() => {
  useUiStore.setState({ selectedRepoId: null });
});

describe("D4 fleet cache applyEngineBatch", () => {
  it("fleet cache: repo stores the snapshot with no sessions and no detection yet", () => {
    apply(repoMsg());
    expect(fleetOf(), "the repo entry").toEqual({ repo: base(), sessions: new Map(), detection: null });
  });

  it("fleet cache: a new repo snapshot replaces the entry and empties the sessions", () => {
    apply(repoMsg(), sessionsMsg([sessionOf("claude-1", "feat-pagos")]));
    expect(fleetOf()?.sessions.size, "the sessions are in").toBe(1);
    const next = { ...base(), name: "tienda-renamed" };
    apply({ type: "repo", fleet: next });
    expect(fleetOf(), "the sessions are asked again after a snapshot").toEqual({ repo: next, sessions: new Map(), detection: null });
  });

  it("fleet cache: worktrees replaces the worktrees and the fetch time and keeps the rest", () => {
    apply(repoMsg(), sessionsMsg([sessionOf("claude-1", "feat-pagos")]));
    const worktrees = base().worktrees.slice(0, 2);
    apply({ type: "worktrees", repoId: "tienda", worktrees, fetchedUtcMs: 123, perf });
    expect(fleetOf()?.repo.worktrees).toEqual(worktrees);
    expect(fleetOf()?.repo.fetchedUtcMs).toBe(123);
    expect(fleetOf()?.repo.name, "the rest of the repo is kept").toBe("tienda");
    expect(fleetOf()?.sessions.size, "the sessions are kept").toBe(1);
    apply({ type: "worktrees", repoId: "tienda", worktrees, fetchedUtcMs: null, perf });
    expect(fleetOf()?.repo.fetchedUtcMs, "a null fetch time replaces too").toBeNull();
  });

  it("fleet cache: messages of a repo that has no entry create none", () => {
    apply({ type: "worktrees", repoId: "tienda", worktrees: [], fetchedUtcMs: null, perf });
    apply(sessionsMsg([sessionOf("claude-1", "feat-pagos")]));
    apply(repoSessionMsg(sessionOf("claude-1", "feat-pagos")));
    apply({ type: "prediction", repoId: "tienda", pairs: [], removed: [], perf });
    expect(client.getQueryCache().findAll({ queryKey: ["engine", "repo"] }), "no entry without a repo snapshot").toHaveLength(0);
    apply(repoMsg());
    expect(fleetOf(), "the earlier messages left nothing behind in the next snapshot").toEqual({ repo: base(), sessions: new Map(), detection: null });
  });

  it("fleet cache: sessions merges the list by sessionId and sets detection", () => {
    apply(repoMsg(), sessionsMsg([sessionOf("claude-1", "feat-pagos"), sessionOf("claude-2", "feat-login")], true));
    expect([...(fleetOf()?.sessions.keys() ?? [])].sort()).toEqual(["claude-1", "claude-2"]);
    expect(fleetOf()?.detection).toBe(true);
    apply(sessionsMsg([sessionOf("claude-3", "tienda")], false));
    expect([...(fleetOf()?.sessions.keys() ?? [])].sort(), "a list merges, it does not replace").toEqual(["claude-1", "claude-2", "claude-3"]);
    expect(fleetOf()?.detection).toBe(false);
  });

  it("fleet cache: the later stateSince wins, and on a tie the ended session wins (D3)", () => {
    const active = sessionOf("claude-1", "feat-pagos", { state: "active", stateSinceUtcMs: 1000 });
    const ended = sessionOf("claude-1", "feat-pagos", { state: "ended", stateSinceUtcMs: 1000, endedUtcMs: 1000, endCause: "process-gone" });
    const newer = sessionOf("claude-1", "feat-pagos", { state: "inactive", stateSinceUtcMs: 2000 });
    const older = sessionOf("claude-1", "feat-pagos", { state: "active", stateSinceUtcMs: 500 });

    apply(repoMsg(), repoSessionMsg(active), repoSessionMsg(ended));
    expect(fleetOf()?.sessions.get("claude-1")?.state, "an ended session with the same stateSince wins").toBe("ended");
    apply(repoSessionMsg(active));
    expect(fleetOf()?.sessions.get("claude-1")?.state, "an active one with the same stateSince does not undo the end").toBe("ended");
    apply(repoSessionMsg(newer));
    expect(fleetOf()?.sessions.get("claude-1")?.state, "a later stateSince wins").toBe("inactive");
    apply(repoSessionMsg(older));
    expect(fleetOf()?.sessions.get("claude-1")?.state, "an earlier stateSince is dropped").toBe("inactive");

    apply(repoMsg(), sessionsMsg([ended]), sessionsMsg([active]));
    expect(fleetOf()?.sessions.get("claude-1")?.state, "the same rule in a list, in any order").toBe("ended");
    apply(repoMsg(), sessionsMsg([active, ended]));
    expect(fleetOf()?.sessions.get("claude-1")?.state, "the same rule inside one list").toBe("ended");
  });

  it("fleet cache: prediction upserts pairs by pairId and removes the removed ones", () => {
    const one = { pairId: "p1", a: "key-feat-login", b: "key-feat-pagos", state: "current", level: "conflict" } as const;
    const two = { pairId: "p2", a: "key-feat-pagos", b: "base", state: "pending", level: null } as const;
    apply(repoMsg(), { type: "prediction", repoId: "tienda", pairs: [one, two], removed: [], perf });
    expect(fleetOf()?.repo.pairs.map((p) => p.pairId).sort()).toEqual(["p1", "p2"]);
    apply({ type: "prediction", repoId: "tienda", pairs: [{ ...one, level: "overlap" }], removed: ["p2"], perf });
    expect(fleetOf()?.repo.pairs, "p1 is updated in place and p2 is gone").toEqual([{ ...one, level: "overlap" }]);
  });

  it("fleet cache: repoClosed removes the entry and deselects the repo if it was the chosen one", () => {
    apply(repoMsg("tienda"), repoMsg("notas"));
    expect(typeof useUiStore.getState().selectRepo, "the store has selectRepo").toBe("function");
    useUiStore.getState().selectRepo("tienda");
    apply({ type: "repoClosed", repoId: "notas" });
    expect(fleetOf("notas"), "the closed repo has no entry").toBeUndefined();
    expect(fleetOf("tienda"), "the other one stays").toBeDefined();
    expect(useUiStore.getState().selectedRepoId, "another repo closing does not deselect").toBe("tienda");
    apply({ type: "repoClosed", repoId: "tienda" });
    expect(fleetOf("tienda")).toBeUndefined();
    expect(useUiStore.getState().selectedRepoId, "the chosen repo closing deselects it").toBeNull();
  });

  it("fleet cache: only the repo of the message changes", () => {
    apply(repoMsg("tienda"), repoMsg("notas"));
    const notas = fleetOf("notas");
    expect(notas, "notas has an entry").toBeDefined();
    apply({ type: "worktrees", repoId: "tienda", worktrees: [], fetchedUtcMs: 5, perf }, sessionsMsg([sessionOf("claude-1", "feat-pagos")]));
    expect(fleetOf("notas"), "notas is the same object").toBe(notas);
  });

  it.each(["worktrees", "sessions", "repoSession", "prediction"] as const)("fleet cache: %s never mutates the previous entry", (variant) => {
    apply(repoMsg(), sessionsMsg([sessionOf("claude-1", "feat-pagos", { stateSinceUtcMs: 1000 })]));
    const previous = fleetOf();
    const sessionsBefore = previous?.sessions;
    const copy = structuredClone(previous);
    const messages: Record<typeof variant, EngineMessage> = {
      worktrees: { type: "worktrees", repoId: "tienda", worktrees: [], fetchedUtcMs: 1, perf },
      sessions: sessionsMsg([sessionOf("claude-2", "feat-login")], false),
      repoSession: repoSessionMsg(sessionOf("claude-1", "feat-pagos", { state: "inactive", stateSinceUtcMs: 2000 })),
      prediction: { type: "prediction", repoId: "tienda", pairs: [{ pairId: "p9", a: "x", b: "base", state: "pending", level: null }], removed: [], perf },
    };
    apply(messages[variant]);
    expect(previous, "the old entry is untouched").toEqual(copy);
    expect(sessionsBefore, "the old sessions map is untouched").toEqual(copy?.sessions);
    expect(fleetOf(), "a new entry is stored").not.toBe(previous);
  });

  it("fleet cache: the repo entry has no network queryFn", () => {
    apply(repoMsg());
    const query = client.getQueryCache().find({ queryKey: engineKeys.repo("tienda") });
    expect(query, "the repo entry exists").toBeDefined();
    expect(query?.options.queryFn, "written by setQueryData only").toBeUndefined();
  });
});

describe("D4 fleet fixture (serialized from wire.rs)", () => {
  it("fleet fixture: repo, worktrees, sessions, repoSession, prediction and repoClosed leave the expected cache", () => {
    const repo = msg("repo");
    const id = repo.fleet.repoId;
    apply(repo);
    expect(fleetOf(id), "repo").toEqual({ repo: repo.fleet, sessions: new Map(), detection: null });

    const worktrees = msg("worktrees");
    apply(worktrees);
    expect(fleetOf(id)?.repo.worktrees, "worktrees replaces the worktrees").toEqual(worktrees.worktrees);
    expect(fleetOf(id)?.repo.fetchedUtcMs, "and the fetch time").toBe(worktrees.fetchedUtcMs);

    const sessions = msg("sessions");
    apply(sessions);
    expect([...(fleetOf(id)?.sessions.keys() ?? [])].sort(), "sessions").toEqual(sessions.sessions.map((s) => s.sessionId).sort());
    expect(fleetOf(id)?.detection, "detection").toBe(sessions.detectionAvailable);

    const repoSession = msg("repoSession");
    apply(repoSession);
    expect(fleetOf(id)?.sessions.get(repoSession.session.sessionId), "repoSession: the later stateSince wins").toEqual(repoSession.session);
    expect(fleetOf(id)?.sessions.size, "repoSession merges one").toBe(sessions.sessions.length);

    const prediction = msg("prediction");
    apply(prediction);
    const pairs = fleetOf(id)?.repo.pairs ?? [];
    const first = prediction.pairs[0];
    expect(pairs.find((p) => p.pairId === first?.pairId), "prediction upserts").toEqual(first);
    expect(
      pairs.map((p) => p.pairId).filter((pairId) => prediction.removed.includes(pairId)),
      "and drops the removed ones",
    ).toEqual([]);
    expect(pairs.length, "the other pairs stay").toBe(repo.fleet.pairs.length);

    const closed = msg("repoClosed");
    apply({ ...repo, fleet: { ...repo.fleet, repoId: closed.repoId } });
    expect(typeof useUiStore.getState().selectRepo, "the store has selectRepo").toBe("function");
    useUiStore.getState().selectRepo(closed.repoId);
    apply(closed);
    expect(fleetOf(closed.repoId), "repoClosed removes the entry").toBeUndefined();
    expect(useUiStore.getState().selectedRepoId, "and deselects it").toBeNull();
  });
});
