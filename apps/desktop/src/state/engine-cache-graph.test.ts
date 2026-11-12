// DS-US-DSK-005 T009 (§ 9.4): the graph entry of the cache. `graph` replaces, `graphUnavailable` keeps the last graph,
// `repoClosed` removes it and a new `repo` snapshot leaves it. Written with the golden fixture shared with Rust.
import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineMessage } from "@/client/engine-types";
import { engineKeys } from "@/client/queries";
import { tienda } from "@/fleet/fixtures";
import fixture from "../../tests/fixtures/engine-messages.json";
import { applyEngineBatch, type GraphEntry } from "./engine-cache";

const F = fixture as unknown as Record<string, EngineMessage>;
const of = <T extends EngineMessage["type"]>(key: string, type: T): Extract<EngineMessage, { type: T }> => {
  const message = F[key];
  if (message?.type !== type) throw new Error(`fixture ${key} is not a ${type}`);
  return message as Extract<EngineMessage, { type: T }>;
};

let client: QueryClient;
beforeEach(() => {
  client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
});
const apply = (...batch: EngineMessage[]) => applyEngineBatch(client, batch);
const entry = (repoId = "repo-alpha") => client.getQueryData<GraphEntry>(engineKeys.graph(repoId));

describe("D9-CACHE graph entry", () => {
  it("graph replaces, repo closed removes it, a new repo snapshot keeps it", () => {
    const graph = of("graph", "graph");
    apply(graph);
    expect(entry()).toEqual({ graph: graph.graph, unavailable: null });

    const next = of("graphBaseMissing", "graph");
    apply(next);
    expect(entry(), "the next graph replaces, never merges").toEqual({ graph: next.graph, unavailable: null });

    apply({ type: "repo", fleet: { ...tienda().repo, repoId: "repo-alpha" } });
    expect(entry(), "a new snapshot of the repo does not drop its graph: the supervisor asks for another").toEqual({ graph: next.graph, unavailable: null });

    apply({ type: "repoClosed", repoId: "repo-alpha" });
    expect(entry()).toBeUndefined();
  });

  it("graph unavailable keeps the last graph", () => {
    const graph = of("graph", "graph");
    apply(graph, of("graphUnavailable", "graphUnavailable"));
    expect(entry()).toEqual({ graph: graph.graph, unavailable: "unsupported" });
    apply(graph);
    expect(entry()?.unavailable, "a new graph clears the reason").toBeNull();
  });

  it("graph unavailable with no graph yet has no graph", () => {
    apply(of("graphUnavailable", "graphUnavailable"));
    expect(entry()).toEqual({ graph: null, unavailable: "unsupported" });
  });

  it("never mutates the previous entry", () => {
    const graph = of("graph", "graph");
    apply(graph);
    const before = entry();
    apply(of("graphUnavailable", "graphUnavailable"));
    expect(entry()).not.toBe(before);
    expect(before?.unavailable).toBeNull();
  });

  it("the graph of another repo is untouched by a close", () => {
    apply(of("graph", "graph"));
    apply({ type: "repoClosed", repoId: "repo-beta" });
    expect(entry()).not.toBeUndefined();
  });
});
