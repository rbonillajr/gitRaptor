// D3-CACHE (DS-US-DSK-003 T006): applyEngineBatch leaves the query cache and the engine store as the Dev Spec says for
// every variant of the golden fixture (`tests/fixtures/engine-messages.json`, serialized from `wire.rs`), never mutates
// the previous object, and writes only with `setQueryData` (no network queryFn, nobody writes `engineKeys.repo`).
import { QueryClient } from "@tanstack/react-query";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineMessage, GlobalDto } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import fixture from "../../tests/fixtures/engine-messages.json";
import { applyEngineBatch } from "./engine-cache";
import { useEngineStore } from "./engine-store";

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
  it("D3-CACHE: the keys are the Dev Spec's and nothing writes engineKeys.repo", () => {
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
    expect(client.getQueryCache().findAll({ queryKey: ["engine", "repo"] }), "no repo entry is written").toHaveLength(0);
    const queries = client.getQueryCache().findAll();
    expect(queries.length, "the batch writes the cache").toBeGreaterThan(0);
    for (const q of queries) expect(q.options.queryFn, `${q.queryHash} must have no network queryFn`).toBeUndefined();

    const root = fileURLToPath(new URL("..", import.meta.url));
    const users = (readdirSync(root, { recursive: true, encoding: "utf8" }) as string[])
      .filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f) && !f.endsWith("client/queries.ts"))
      .filter((f) => readFileSync(`${root}/${f}`, "utf8").includes("engineKeys.repo"));
    expect(users, "engineKeys.repo has no writers in this story").toEqual([]);
  });
});
