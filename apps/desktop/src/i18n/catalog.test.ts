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

  it("FE3: keys are lowerCamel per segment (plural suffixes _one/_other allowed)", () => {
    for (const [key] of enEntries) {
      for (const segment of key.split(".")) expect(segment, `key ${key}`).toMatch(/^[a-z][a-zA-Z0-9]*(_one|_other)?$/);
    }
  });
});

// TS003-I18N (TS-DSK-003 T002): the 21 ui.* keys of the Dev Spec (§ Claves i18n).
const UI_KEYS = [
  "ui.common.close", "ui.common.cancel", "ui.common.loading", "ui.button.busy", "ui.chip.remove",
  "ui.pill.reversible", "ui.pill.confirm", "ui.pill.irreversible", "ui.pill.live",
  "ui.status.active", "ui.status.inactive", "ui.status.conflict", "ui.status.live",
  "ui.tree.loading", "ui.toast.region", "ui.toast.kind.success", "ui.toast.kind.warning", "ui.toast.kind.error",
  "ui.tooltip.shortcut", "ui.table.settings", "ui.table.loading",
];

describe("TS003-I18N ui.* keys", () => {
  const entries = { en: flatten(en), es: flatten(es) };
  const text = (lang: "en" | "es", k: string) => String(entries[lang].find(([key]) => key === k)?.[1] ?? "");

  it("TS003-I18N: en and es carry the 21 ui.* keys of the primitives", () => {
    expect(UI_KEYS).toHaveLength(21);
    for (const lang of ["en", "es"] as const) {
      const keys = new Set(entries[lang].map(([k]) => k));
      for (const key of UI_KEYS) expect(keys.has(key), `${lang} lacks ${key}`).toBe(true);
    }
  });

  it("TS003-I18N: interpolated keys keep their placeholders in both languages", () => {
    for (const lang of ["en", "es"] as const) {
      expect(text(lang, "ui.chip.remove"), `${lang}: ui.chip.remove needs {{label}}`).toContain("{{label}}");
      expect(text(lang, "ui.tooltip.shortcut"), `${lang}: ui.tooltip.shortcut needs {{keys}}`).toContain("{{keys}}");
    }
  });

  it("TS003-I18N: the texts the spec quotes (busy, recoverable)", () => {
    expect(text("en", "ui.button.busy")).toBe("In progress");
    expect(text("es", "ui.button.busy")).toBe("En curso");
    expect(text("en", "ui.pill.reversible")).toBe("Recoverable");
    expect(text("es", "ui.pill.reversible")).toBe("Recuperable");
  });
});
