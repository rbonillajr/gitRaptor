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

// D7-GATE (DS-US-DSK-007 § Textos del catálogo): the grd.* keys in en and es with the texts of the Dev Spec.
const GRD_KEYS: Record<string, [string, string]> = {
  "grd.nav.title": ["Guardrails", "Guardrails"],
  "grd.nav.unseen_one": ["{{count}} new block", "{{count}} bloqueo nuevo"],
  "grd.nav.unseen_other": ["{{count}} new blocks", "{{count}} bloqueos nuevos"],
  "grd.nav.labelUnseen": ["Guardrails, {{unseen}}", "Guardrails, {{unseen}}"],
  "grd.view.back": ["Back to the fleet", "Volver a la flota"],
  "grd.summary": ["{{blocked}} blocked in the last {{days}} days · {{exceptions}} exceptions", "{{blocked}} bloqueadas en los últimos {{days}} días · {{exceptions}} excepciones"],
  "grd.empty": ["Nothing blocked in {{repo}}. Protection is on.", "Nada bloqueado en {{repo}}. La protección está activa."],
  "grd.empty.unprotected": ["{{repo}} isn't protected.", "{{repo}} no está protegido."],
  "grd.unverified": ["Logged without detail (too many at once)", "Anotadas sin detalle (demasiadas a la vez)"],
  "grd.unlogged": ["From {{from}} to {{to}} the engine wasn't running: what the hooks blocked then isn't logged", "De {{from}} a {{to}} el motor no estaba en marcha: lo que bloquearon los hooks entonces no está anotado"],
  "grd.unavailable.active": ["The block list of {{repo}} isn't available right now. Protection is still on: the hooks decide without this list.", "La lista de bloqueos de {{repo}} no está disponible ahora. La protección sigue activa: los hooks deciden sin esta lista."],
  "grd.unavailable.unknown": ["The block list of {{repo}} isn't available right now. Protection doesn't depend on this list: if {{repo}} is protected, the hooks keep stopping operations.", "La lista de bloqueos de {{repo}} no está disponible ahora. La protección no depende de esta lista: si {{repo}} está protegido, los hooks siguen frenando."],
  "grd.unavailable.notObserved": ["GitRaptor isn't watching {{repo}} right now.", "GitRaptor no está observando {{repo}} ahora."],
  "grd.stale": ["Last read {{age}} ago", "Última lectura hace {{age}}"],
  "grd.block.title": ["Blocked: {{actor}} tried to {{operation}}", "Bloqueado: {{actor}} intentó {{operation}}"],
  "grd.block.titleNoActor": ["Blocked: someone tried to {{operation}} (no agent identified)", "Bloqueado: se intentó {{operation}} (sin agente identificado)"],
  "grd.agent.claudeCode": ["Claude Code", "Claude Code"],
  "grd.agent.other": ["another agent", "otro agente"],
  "grd.agent.none": ["an agent", "un agente"],
  "grd.actor.claudeCode": ["Claude Code · {{worktree}}", "Claude Code · {{worktree}}"],
  "grd.actor.other": ["another agent · {{worktree}}", "otro agente · {{worktree}}"],
  "grd.op.forcePush": ["rewrite the history of {{ref}} on the server ({{remote}})", "reescribir el historial de {{ref}} en el servidor ({{remote}})"],
  "grd.op.pushDelete": ["delete {{ref}} on the server ({{remote}})", "borrar {{ref}} en el servidor ({{remote}})"],
  "grd.op.push": ["push to {{ref}} on the server ({{remote}})", "subir a {{ref}} en el servidor ({{remote}})"],
  "grd.op.branchDelete": ["delete the branch {{ref}}", "borrar la rama {{ref}}"],
  "grd.op.refMove": ["move the branch {{ref}}", "mover la rama {{ref}}"],
  "grd.op.rebase": ["rebase {{branch}}", "rebasar {{branch}}"],
  "grd.op.rebaseUnknown": ["a rebase", "un rebase"],
  "grd.op.commit": ["make a commit on {{branch}}", "hacer un commit en {{branch}}"],
  "grd.block.nothing": ["Nothing happened. {{ref}} is unchanged on your machine and on the server.", "No pasó nada. {{ref}} sigue igual en tu equipo y en el servidor."],
  "grd.block.nothingLocal": ["Nothing happened. {{ref}} is unchanged on your machine.", "No pasó nada. {{ref}} sigue igual en tu equipo."],
  "grd.block.attempted": ["What it tried", "Qué intentó"],
  "grd.block.times": ["×{{count}}, the last at {{time}}", "×{{count}}, la última a las {{time}}"],
  "grd.block.whyLabel": ["Why it's dangerous", "Por qué es peligroso"],
  "grd.block.stoppedLabel": ["What stopped it", "Qué lo frenó"],
  "grd.block.canDo": ["What you can do", "Qué puedes hacer"],
  "grd.block.technical": ["Technical details", "Detalles técnicos"],
  "grd.block.ok": ["Got it", "Entendido"],
  "grd.block.seeLog": ["See in the log", "Ver en el registro"],
  "grd.exc.cta": ["I'll do it myself…", "Lo haré yo…"],
  "grd.exc.terminal.title": ["Confirm from your terminal", "Confirma desde tu terminal"],
  "grd.exc.who": ["You do it, not {{agent}}. It runs once; the rule stays on afterwards.", "La harás tú, no {{agent}}. Se ejecuta una sola vez; la regla sigue activa después."],
  "grd.exc.where": ["Where's the terminal? Press Cmd+Space and type \"Terminal\".", "¿Dónde está la terminal? Pulsa Cmd+Espacio y escribe \"Terminal\"."],
  "grd.exc.mismatch": ["The announced exception isn't the one you copied: cancel it with Ctrl-C in that terminal.", "La excepción anunciada no es la que copiaste: cancélala con Ctrl-C en esa terminal."],
  "grd.toast.kindLabel": ["Blocked", "Bloqueo"],
  "grd.exc.cancelHow": ["Once you run it you have 10 seconds to cancel: press Ctrl-C in that terminal.", "Cuando lo ejecutes tendrás 10 segundos para cancelar: pulsa Ctrl-C en esa terminal."],
  "grd.exc.cancelOr": ["Or run this in another terminal:", "O ejecuta esto en otra terminal:"],
  "grd.exc.pendingNow": ["Running now…", "Ejecutándose ahora…"],
  "grd.exc.close": ["Close", "Cerrar"],
  "grd.exc.pending": ["Exception announced in a terminal: runs in {{n}} s", "Excepción anunciada en una terminal: se ejecuta en {{n}} s"],
  "grd.exc.done": ["Done with an exception: {{operation}}; skipped {{rules}}", "Hecho con excepción: {{operation}}; se saltó {{rules}}"],
  "grd.exc.cancelled": ["Cancelled: nothing ran. It's in the log.", "Cancelada: no se ejecutó nada. Queda en el registro."],
  "grd.exc.cancelledBy": ["Cancelled by {{actor}}: nothing ran.", "La canceló {{actor}}: no se ejecutó nada."],
  "grd.exc.rejected": ["Exception refused: {{operation}}", "Excepción rechazada: {{operation}}"],
  "grd.exc.engineDown": ["The engine is stopped: the exception needs it.", "El motor está parado: la excepción lo necesita."],
  "grd.exc.notOffered.commit": ["The exception only works for push, deleting a local branch and rebase.", "La excepción solo sirve para push, borrar una rama local y rebase."],
  "grd.exc.notOffered.operation": ["The exception only works for push, deleting a local branch and rebase.", "La excepción solo sirve para push, borrar una rama local y rebase."],
  "grd.exc.notOffered.rule": ["This rule can't be skipped with an exception.", "Esta regla no se puede saltar con una excepción."],
  "grd.exc.notOffered.remote": ["The exception needs a remote by name, not a URL.", "La excepción necesita un remoto por su nombre, no una URL."],
  "grd.exc.notOffered.whereUnknown": ["GitRaptor didn't log where it happened, so it can't write the command.", "GitRaptor no anotó dónde ocurrió, así que no puede escribir el comando."],
  "grd.exc.notOffered.rateLimited": ["GitRaptor didn't log where it happened, so it can't write the command.", "GitRaptor no anotó dónde ocurrió, así que no puede escribir el comando."],
  "grd.exc.notOffered.unsafeText": ["A name in this operation has characters that could act on your terminal, so we don't offer the command.", "Un nombre de esta operación tiene caracteres que podrían actuar sobre tu terminal, así que no ofrecemos el comando."],
  "grd.exc.notOffered.raptorUnknown": ["GitRaptor can't find its own `raptor` to write the command.", "GitRaptor no encuentra su propio `raptor` para escribir el comando."],
  "grd.toast.many": ["{{count}} operations were blocked", "Se bloquearon {{count}} operaciones"],
  "grd.gloss.ask": ["What is {{term}}?", "¿Qué es {{term}}?"],
  "grd.block.openLabel": ["View the block: {{title}}", "Ver el bloqueo: {{title}}"],
};
const GRD_LEVELS = ["minimum", "floor", "worktree", "profile", "local", "system"];
const GRD_LEVEL_TEXT: Record<string, [string, string]> = {
  floor: ["team configuration", "configuración del equipo"],
  worktree: ["team configuration on this branch", "configuración del equipo en esta rama"],
  profile: ["personal configuration", "configuración personal"],
  local: ["local configuration", "configuración local"],
  system: ["default value", "valor por defecto"],
};
const GRD_RULE_TEXT: Record<string, [string, string]> = {
  "force-push": ["no force-push", "prohibir force-push"],
  "base-branch-delete": ["base branch protection", "protección de la rama base"],
  "protected-branch": ["protected branch", "rama protegida"],
  "forbidden-path": ["forbidden path", "ruta prohibida"],
  "human-author": ["commits are made by the person (human-author)", "los commits los hace la persona (human-author)"],
};
const NOT_OFFERED = ["commit", "operation", "rule", "remote", "whereUnknown", "rateLimited", "unsafeText", "raptorUnknown"];

describe("D7-GATE guardrails keys", () => {
  const entries = { en: new Map(flatten(en)), es: new Map(flatten(es)) };

  it("D7-GATE: has the grd.* keys in en and es with the Dev Spec texts", () => {
    for (const lang of ["en", "es"] as const) {
      const index = lang === "en" ? 0 : 1;
      for (const [key, texts] of Object.entries(GRD_KEYS)) {
        expect(entries[lang].get(key), `${lang} lacks ${key} or its text differs from the Dev Spec`).toBe(texts[index]);
      }
    }
  });

  it("D7-GATE: every level has a text in en and es, the ones of guard.txt", () => {
    for (const lang of ["en", "es"] as const) {
      const index = lang === "en" ? 0 : 1;
      for (const level of GRD_LEVELS) {
        const value = entries[lang].get(`grd.level.${level}`);
        expect(typeof value === "string" && value.trim().length > 0, `${lang} lacks grd.level.${level}`).toBe(true);
        const expected = GRD_LEVEL_TEXT[level];
        if (expected) expect(value, `${lang} grd.level.${level} copies guard.level.${level}`).toBe(expected[index]);
      }
    }
  });

  it("D7-GATE: every rule of the wire has a text in en and es, copied from guard.log.rule.*", () => {
    for (const lang of ["en", "es"] as const) {
      const index = lang === "en" ? 0 : 1;
      const rules = [...entries[lang].entries()].filter(([k]) => k.startsWith("grd.rule."));
      expect(rules.length, `${lang}: one text per rule of guard.log.rule.* (13)`).toBeGreaterThanOrEqual(13);
      for (const [short, texts] of Object.entries(GRD_RULE_TEXT)) {
        expect(
          rules.some(([, v]) => v === texts[index]),
          `${lang}: a grd.rule.* key must say "${texts[index]}" (guard.log.rule.${short})`,
        ).toBe(true);
      }
    }
  });

  it("D7-GATE: every reason of NotOffered that the app words has a text", () => {
    for (const lang of ["en", "es"] as const) {
      for (const reason of NOT_OFFERED) {
        const value = entries[lang].get(`grd.exc.notOffered.${reason}`);
        expect(typeof value === "string" && value.trim().length > 0, `${lang} lacks grd.exc.notOffered.${reason}`).toBe(true);
      }
    }
  });

  it("D7-GATE: the long texts of the Dev Spec exist in both languages", () => {
    for (const lang of ["en", "es"] as const) {
      for (const key of ["grd.exc.reason", "grd.exc.remote", "grd.exc.terminal", "grd.exc.review", "grd.exc.sourceInferred", "grd.exc.branchDeleteHint", "grd.exc.rebaseHint", "grd.block.why.forcepush", "grd.block.why.deletebase", "grd.block.do.nothing", "grd.block.do.exception", "grd.block.attemptedPush"]) {
        const value = entries[lang].get(key);
        expect(typeof value === "string" && value.trim().length > 0, `${lang} lacks ${key}`).toBe(true);
      }
      expect(entries[lang].get("grd.exc.sourceInferred"), `${lang}: grd.exc.sourceInferred takes {{branch}}`).toContain("{{branch}}");
    }
  });
});
