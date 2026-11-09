// FE3 (TS-DSK-001 T006): the en and es catalogs have the same key set and no empty value.
import { describe, expect, it } from "vitest";
import en from "./locales/en.json";
import es from "./locales/es.json";

function flatten(node: unknown, prefix = ""): [string, unknown][] {
  if (node !== null && typeof node === "object" && !Array.isArray(node)) {
    return Object.entries(node).flatMap(([k, v]) => flatten(v, prefix ? `${prefix}.${k}` : k));
  }
  return [[prefix, node]];
}

describe("FE3 catalogs", () => {
  const enEntries = flatten(en);
  const esEntries = flatten(es);

  it("FE3: en and es have the same keys", () => {
    expect(enEntries.length, "the en catalog is empty").toBeGreaterThan(0);
    expect(esEntries.map(([k]) => k).sort()).toEqual(enEntries.map(([k]) => k).sort());
  });

  it("FE3: no value is empty", () => {
    const all = [...enEntries.map(([k, v]) => [`en:${k}`, v] as const), ...esEntries.map(([k, v]) => [`es:${k}`, v] as const)];
    for (const [key, value] of all) {
      expect(typeof value === "string" && value.trim().length > 0, `${key} is empty or not a string`).toBe(true);
    }
  });

  it("FE3: keys are lowerCamel per segment", () => {
    for (const [key] of enEntries) {
      for (const segment of key.split(".")) expect(segment, `key ${key}`).toMatch(/^[a-z][a-zA-Z0-9]*$/);
    }
  });
});
