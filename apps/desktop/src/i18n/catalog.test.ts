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

// D8-GATE (DS-US-DSK-008 § Textos del catálogo): the predicted-conflict texts in en and es.
const CONFLICT_KEYS: Record<string, [string, string]> = {
  "conflicts.panel.title": ["Predicted conflicts", "Conflictos previstos"],
  "conflicts.panel.list": ["Predicted conflicts in {{repo}}", "Conflictos previstos en {{repo}}"],
  "conflicts.band.conflict": ["⚡ predicted conflict", "⚡ conflicto previsto"],
  "conflicts.band.overlap": ["⚠ same files, not committed yet", "⚠ mismos archivos sin guardar en commit"],
  "conflicts.band.pair": ["{{a}} ↔ {{b}}", "{{a}} ↔ {{b}}"],
  "conflicts.band.more_one": ["and {{count}} more", "y {{count}} más"],
  "conflicts.band.more_other": ["and {{count}} more", "y {{count}} más"],
  "conflicts.band.moreTruncated": ["and more", "y más"],
  "conflicts.band.new": ["new", "nuevo"],
  "conflicts.band.open": ["View", "Ver"],
  "conflicts.band.openLabel": ["View the conflict between {{a}} and {{b}}", "Ver el conflicto entre {{a}} y {{b}}"],
  "conflicts.band.a11y": ["{{level}} between {{a}} and {{b}}, {{file}}, {{freshness}}", "{{level}} entre {{a}} y {{b}}, {{file}}, {{freshness}}"],
  "conflicts.side.base": ["{{base}} (main branch)", "{{base}} (rama principal)"],
  "conflicts.freshness.ago": ["{{age}} ago", "hace {{age}}"],
  "conflicts.freshness.stale": ["outdated · {{age}} ago · recalculating", "desactualizado · hace {{age}} · recalculando"],
  "conflicts.freshness.clear": ["No predicted conflicts · {{age}} ago", "Sin conflictos previstos · hace {{age}}"],
  "conflicts.freshness.calculating_one": ["Calculating {{n}} of 1 pair", "Calculando {{n}} de 1 par"],
  "conflicts.freshness.calculating_other": ["Calculating {{n}} of {{count}} pairs", "Calculando {{n}} de {{count}} pares"],
  "conflicts.freshness.calculatingSince_one": ["Calculating {{n}} of 1 pair · last result {{age}} ago", "Calculando {{n}} de 1 par · último resultado hace {{age}}"],
  "conflicts.freshness.calculatingSince_other": ["Calculating {{n}} of {{count}} pairs · last result {{age}} ago", "Calculando {{n}} de {{count}} pares · último resultado hace {{age}}"],
  "conflicts.freshness.pending_one": ["Pending: {{reason}} (1 pair)", "Pendiente: {{reason}} (1 par)"],
  "conflicts.freshness.pending_other": ["Pending: {{reason}} ({{count}} pairs)", "Pendiente: {{reason}} ({{count}} pares)"],
  "conflicts.freshness.notComputable_one": ["Not computable: {{reason}} (1 pair)", "No calculable: {{reason}} (1 par)"],
  "conflicts.freshness.notComputable_other": ["Not computable: {{reason}} ({{count}} pairs)", "No calculable: {{reason}} ({{count}} pares)"],
  "conflicts.base.pending": ["Base branch waiting for confirmation ({{base}})", "Rama base pendiente de confirmar ({{base}})"],
  "conflicts.base.hint": [
    "Run it in your own terminal (not an agent's). It shows what it will do and asks before doing it.",
    "Ejecútalo en tu propia terminal (no en la de un agente). Te enseñará qué va a hacer y te preguntará antes.",
  ],
  "conflicts.base.hintFolder": [
    "Run it in your own terminal, in the folder of {{repo}}. It shows what it will do and asks before doing it.",
    "Ejecútalo en tu propia terminal, en la carpeta de {{repo}}. Te enseñará qué va a hacer y te preguntará antes.",
  ],
  "conflicts.limits.open": ["Limits (?)", "Límites (?)"],
  "conflicts.limits.title": ["Prediction limits", "Límites de la predicción"],
  "conflicts.range.line": ["line {{start}}", "línea {{start}}"],
  "conflicts.range.lines": ["lines {{start}}–{{end}}", "líneas {{start}}–{{end}}"],
  "conflicts.range.more": ["and more ranges", "y más rangos"],
  "conflicts.range.a11y": ["lines {{start}} to {{end}}", "líneas {{start}} a {{end}}"],
  "conflicts.detail.title": ["⚡ {{a}} ↔ {{b}} are going to collide", "⚡ {{a}} ↔ {{b}} van a chocar"],
  "conflicts.detail.titleOverlap": ["⚠ {{a}} and {{b}} are editing the same files", "⚠ {{a}} y {{b}} están editando los mismos archivos"],
  "conflicts.detail.titleBase": ["⚡ {{a}} is going to collide with {{base}}", "⚡ {{a}} va a chocar con {{base}}"],
  "conflicts.detail.computed": ["{{a}} ↔ {{b}} · computed {{age}} ago", "{{a}} ↔ {{b}} · calculado hace {{age}}"],
  "conflicts.detail.back": ["Back to the fleet", "Volver a la flota"],
  "conflicts.detail.calculating": ["Calculating…", "Calculando…"],
  "conflicts.detail.gone": ["This pair is no longer predicted.", "Este par ya no se calcula."],
  "conflicts.toast.noFile": ["⚡ Predicted conflict: {{a}} ↔ {{b}}", "⚡ Conflicto previsto: {{a}} ↔ {{b}}"],
  "conflicts.toast.withFile_one": ["⚡ Predicted conflict: {{a}} ↔ {{b}} in {{file}}", "⚡ Conflicto previsto: {{a}} ↔ {{b}} en {{file}}"],
  "conflicts.toast.withFile_other": [
    "⚡ Predicted conflict: {{a}} ↔ {{b}} in {{file}} and {{count}} more",
    "⚡ Conflicto previsto: {{a}} ↔ {{b}} en {{file}} y {{count}} más",
  ],
  "conflicts.toast.otherRepo": ["⚡ New predicted conflict in {{repo}}", "⚡ Nuevo conflicto previsto en {{repo}}"],
  "conflicts.toast.many": ["⚡ {{count}} new predicted conflicts", "⚡ {{count}} nuevos conflictos previstos"],
  "conflicts.toast.open": ["View", "Ver"],
  "conflicts.pill_one": ["⚡ {{count}} predicted conflict", "⚡ {{count}} conflicto previsto"],
  "conflicts.pill_other": ["⚡ {{count}} predicted conflicts", "⚡ {{count}} conflictos previstos"],
  "conflicts.pill.label": ["{{count}} predicted conflicts across your repos. Open them", "{{count}} conflictos previstos en tus repos. Abrirlos"],
  "conflicts.pill.compact": ["⚡ {{count}}", "⚡ {{count}}"],
  "ui.common.copied": ["Copied", "Copiado"],
  "ui.common.copyFailed": ["Couldn't copy. Select the command and copy it by hand.", "No se pudo copiar. Selecciona el comando y cópialo a mano."],
};
const REASONS = ["baseUnconfirmed", "operationInProgress", "powerSaving", "dormant", "baseMissing", "missingObject", "shallow", "limitExceeded", "error"];
const LIMITS = ["overlapIncludesUncommitted", "conflictCommittedOnly", "noMergeDriversOrAttributes", "renameLimit", "insertionsMayMerge", "joinedHunks", "pairIgnoresBaseMoves", "localOnly"];
const KINDS = ["content", "addAdd", "modifyDelete", "rename", "directoryFile", "binary", "submodule"];

describe("D8-GATE predicted conflict keys", () => {
  it("has the predicted conflict keys in en and es", () => {
    const entries = { en: new Map(flatten(en)), es: new Map(flatten(es)) };
    for (const lang of ["en", "es"] as const) {
      const index = lang === "en" ? 0 : 1;
      for (const [key, texts] of Object.entries(CONFLICT_KEYS)) {
        expect(entries[lang].get(key), `${lang} lacks ${key} or its text differs from the Dev Spec`).toBe(texts[index]);
      }
      const nested = [
        ...REASONS.map((r) => `conflicts.reason.${r}`),
        ...LIMITS.map((l) => `conflicts.limit.${l}`),
        ...KINDS.map((k) => `conflicts.kind.${k}`),
        "conflicts.a11y.conflict",
        "conflicts.a11y.overlap",
        "conflicts.tooltip.conflict",
        "conflicts.tooltip.overlap",
        "conflicts.detail.sideA11y",
        "conflicts.detail.todo",
        "conflicts.detail.todoTitle",
        "conflicts.limits.line",
        "conflicts.panel.less",
        "conflicts.panel.more_one",
        "conflicts.panel.more_other",
      ];
      for (const key of nested) {
        const value = entries[lang].get(key);
        expect(typeof value === "string" && value.trim().length > 0, `${lang} lacks ${key}`).toBe(true);
      }
    }
  });
});
