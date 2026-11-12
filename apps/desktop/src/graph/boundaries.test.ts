// DS-US-DSK-005 T011 (D13): src/graph reaches into src/conflicts only through its public module, and never touches
// the wire form of a pair. The check reads the sources, so a new import breaks it the day it is written.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const graphDir = new URL(".", import.meta.url).pathname;

const walk = (dir: string): string[] => readdirSync(dir).flatMap((name) => (statSync(join(dir, name)).isDirectory() ? walk(join(dir, name)) : [join(dir, name)]));
const sources = () => walk(graphDir).filter((p) => /\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p) && !p.endsWith("fixtures.ts"));
const importsOf = (code: string) => [...code.matchAll(/(?:import|export)\s[^;]*?from\s+["']([^"']+)["']/g)].map((m) => m[1] ?? "");

describe("graph boundaries (D13)", () => {
  it("src/graph imports only the public module of src/conflicts", () => {
    const offenders = sources().flatMap((file) =>
      importsOf(readFileSync(file, "utf8"))
        .filter((spec) => /(^|\/)conflicts(\/|$)/.test(spec))
        .filter((spec) => !/^@\/conflicts(\/index)?$/.test(spec))
        .map((spec) => `${relative(graphDir, file)} imports ${spec}`),
    );
    expect(offenders).toEqual([]);
  });

  it("src/graph never imports PairDto", () => {
    const offenders = sources().filter((file) => /\bPairDto\b/.test(readFileSync(file, "utf8"))).map((file) => relative(graphDir, file));
    expect(offenders).toEqual([]);
  });

  it("the scanner sees a private import", () => {
    expect(importsOf('import { predictionView } from "@/conflicts/prediction-model";')).toEqual(["@/conflicts/prediction-model"]);
    expect(importsOf('export type { A } from "@/conflicts";')).toEqual(["@/conflicts"]);
  });
});
