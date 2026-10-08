#!/usr/bin/env node
// Release status, generated from the story cards (fichas) instead of a board. Reads every US, TS,
// INF, SPIKE and TD card under docs/requirements/features/<feature>/{user-stories,technical-stories}/
// and writes docs/requirements/release-status.md.
//
//   node tools/status/release-status.mjs            # write docs/requirements/release-status.md
//   node tools/status/release-status.mjs --check    # exit 1 if the committed file is stale
//   node tools/status/release-status.mjs --json     # print the model as JSON on stdout
//
// Options: --root <repo dir> (default: this repo), --plan <file> (default:
// docs/requirements/release-plan.md under the root), --out <file> (default:
// docs/requirements/release-status.md under the root).
//
// What each card contributes:
// - Frontmatter: `id`, `title`, `type`, `status`, `updated` and, optionally, `milestone` (M1, M2…).
// - The last "## Estado de la implementación (…)" section: its "Implementado en: …" line (the PR
//   numbers) and the "- " bullets that follow a line ending in "Pendiente:" (what is left).
//
// Milestones: a card's `milestone` frontmatter wins. Otherwise, when the release plan exists, a
// card belongs to the first milestone whose section lists its id in a table row. A milestone
// section is a `##`/`###` heading that starts with "M<n>" or "Hito M<n>"; it ends at the next
// heading of the same or higher level. Prose mentions ("fuera de M1") do not count: only rows.
// Without a plan and without `milestone` fields, the report groups by feature and status only.
//
// `--check` (and the write) also fail on a status outside STATUS_ORDER, a likely typo; a status
// section without its "Implementado en:" line is reported as a warning in the file.
//
// The output is deterministic (no clock): the check compares it byte for byte. Read-only except
// for the output file.
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const STORY_DIRS = ['user-stories', 'technical-stories'];
const TYPES = ['us', 'ts', 'inf', 'spike', 'td'];
const ID_RE = /\b(?:US|TS|TD|INF|SPIKE)-[A-Z]+-\d{3}\b/g;

// Report order; unknown statuses follow, alphabetically.
const STATUS_ORDER = ['implemented', 'done', 'partially-implemented', 'ready', 'draft', 'blocked'];
const STATUS_LABEL = {
  implemented: 'Implementado',
  done: 'Hecho',
  'partially-implemented': 'Implementado en parte',
  ready: 'Listo',
  draft: 'Borrador',
  blocked: 'Bloqueado',
};

const REGENERATE = 'node tools/status/release-status.mjs';

/** Top-level scalar keys of a YAML frontmatter block. Nested keys and lists are ignored. */
export function parseFrontmatter(text) {
  const m = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(text);
  if (!m) return null;
  const data = {};
  for (const line of m[1].split(/\r?\n/)) {
    const kv = /^([A-Za-z_][\w-]*):\s*(.*?)\s*$/.exec(line);
    if (!kv || kv[2] === '') continue;
    let value = kv[2];
    if (/^".*"$/.test(value)) value = value.slice(1, -1).replace(/\\"/g, '"');
    else if (/^'.*'$/.test(value)) value = value.slice(1, -1).replace(/''/g, "'");
    else value = value.replace(/\s+#.*$/, '');
    data[kv[1]] = value;
  }
  return data;
}

/** PRs and pending items from the last "## Estado de la implementación" section of a card. */
export function parseImplementation(text) {
  const lines = text.split(/\r?\n/);
  let start = -1;
  for (let i = 0; i < lines.length; i++) {
    if (/^##\s+Estado de la implementaci[oó]n\b/i.test(lines[i])) start = i;
  }
  if (start < 0) return { hasSection: false, implementedIn: null, prs: [], pending: [] };
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^#{1,2}\s/.test(lines[i])) {
      end = i;
      break;
    }
  }
  const section = lines.slice(start + 1, end);
  let implementedIn = null;
  const pending = [];
  for (let i = 0; i < section.length; i++) {
    const impl = /^Implementado en:\s*(.*)$/i.exec(section[i].trim());
    if (impl && implementedIn === null) implementedIn = impl[1].trim();
    if (/Pendiente:\s*$/i.test(section[i].trim())) {
      for (let j = i + 1; j < section.length; j++) {
        const bullet = /^\s*[-*]\s+(.*)$/.exec(section[j]);
        if (!bullet) break;
        pending.push(bullet[1].trim());
      }
    }
  }
  const prs = implementedIn
    ? [...new Set([...implementedIn.matchAll(/#(\d+)/g)].map((x) => Number(x[1])))]
    : [];
  return { hasSection: true, implementedIn, prs, pending };
}

/** Milestone of each id listed in a table row of a "M<n>" section of the release plan. */
export function parseReleasePlan(text) {
  const byId = new Map();
  const order = [];
  let current = null;
  for (const line of text.split(/\r?\n/)) {
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) {
      const level = h[1].length;
      const ms = /^(?:Hito\s+)?(M\d+)\b/i.exec(h[2].trim());
      if (ms && level >= 2 && level <= 3) {
        current = { name: ms[1].toUpperCase(), level };
        if (!order.includes(current.name)) order.push(current.name);
      } else if (current && level <= current.level) {
        current = null;
      }
      continue;
    }
    if (!current || !/^\s*\|/.test(line)) continue;
    for (const id of line.match(ID_RE) ?? []) {
      if (!byId.has(id)) byId.set(id, current.name);
    }
  }
  return { byId, order };
}

function storyFiles(featuresDir) {
  if (!existsSync(featuresDir)) return [];
  const out = [];
  for (const feature of readdirSync(featuresDir, { withFileTypes: true })) {
    if (!feature.isDirectory()) continue;
    for (const sub of STORY_DIRS) {
      const dir = join(featuresDir, feature.name, sub);
      if (!existsSync(dir)) continue;
      for (const f of readdirSync(dir, { withFileTypes: true })) {
        if (f.isFile() && f.name.endsWith('.md')) out.push({ feature: feature.name, file: join(dir, f.name) });
      }
    }
  }
  return out;
}

function compareStatus(a, b) {
  const ia = STATUS_ORDER.indexOf(a);
  const ib = STATUS_ORDER.indexOf(b);
  if (ia >= 0 && ib >= 0) return ia - ib;
  if (ia >= 0) return -1;
  if (ib >= 0) return 1;
  return a.localeCompare(b);
}

function compareMilestone(a, b) {
  return Number(a.slice(1)) - Number(b.slice(1)) || a.localeCompare(b);
}

function countByStatus(items) {
  const counts = {};
  for (const it of items) counts[it.status] = (counts[it.status] ?? 0) + 1;
  return Object.fromEntries(Object.entries(counts).sort(([a], [b]) => compareStatus(a, b)));
}

/** Builds the status model from the cards under <root>/docs/requirements/features. */
export function buildModel({ root, planPath }) {
  const reqDir = join(root, 'docs', 'requirements');
  const plan = planPath && existsSync(planPath) ? parseReleasePlan(readFileSync(planPath, 'utf8')) : null;
  const items = [];
  const warnings = [];
  const errors = [];
  for (const { feature, file } of storyFiles(join(reqDir, 'features'))) {
    const rel = relative(reqDir, file).split(sep).join('/');
    const text = readFileSync(file, 'utf8');
    const fm = parseFrontmatter(text);
    if (!fm || !fm.id) {
      warnings.push(`${rel}: sin frontmatter con id`);
      continue;
    }
    const type = (fm.type ?? '').toLowerCase();
    if (!TYPES.includes(type)) continue;
    const impl = parseImplementation(text);
    const status = fm.status ?? 'unknown';
    if (!STATUS_ORDER.includes(status)) {
      errors.push(`${rel}: status desconocido "${status}" (válidos: ${STATUS_ORDER.join(', ')})`);
    }
    if (impl.hasSection && impl.implementedIn === null) {
      warnings.push(`${rel}: la sección "Estado de la implementación" no tiene la línea "Implementado en:"`);
    }
    items.push({
      id: fm.id,
      title: fm.title ?? '',
      type,
      status,
      feature,
      milestone: fm.milestone ? fm.milestone.toUpperCase() : (plan?.byId.get(fm.id) ?? null),
      updated: fm.updated ?? null,
      path: rel,
      implementedIn: impl.implementedIn,
      prs: impl.prs,
      pending: impl.pending,
    });
  }
  items.sort((a, b) => a.feature.localeCompare(b.feature) || a.id.localeCompare(b.id));

  const features = [...new Set(items.map((i) => i.feature))].sort().map((name) => {
    const own = items.filter((i) => i.feature === name);
    return { name, total: own.length, byStatus: countByStatus(own) };
  });

  const milestoneNames = [...new Set(items.map((i) => i.milestone).filter(Boolean))].sort(compareMilestone);
  const milestones = milestoneNames.map((name) => {
    const own = items.filter((i) => i.milestone === name);
    return { name, total: own.length, byStatus: countByStatus(own) };
  });

  const dates = items.map((i) => i.updated).filter((d) => /^\d{4}-\d{2}-\d{2}$/.test(d ?? '')).sort();
  return {
    generatedBy: REGENERATE,
    lastUpdated: dates.at(-1) ?? null,
    releasePlan: plan ? relative(root, planPath).split(sep).join('/') : null,
    totals: { total: items.length, byStatus: countByStatus(items) },
    features,
    milestones,
    items,
    warnings,
    errors,
  };
}

function cell(text) {
  return String(text ?? '').replace(/\|/g, '\\|').replace(/\r?\n/g, ' ');
}

function statusCell(status) {
  return STATUS_LABEL[status] ? `${STATUS_LABEL[status]} (\`${status}\`)` : `\`${status}\``;
}

function statusSummary(byStatus) {
  return Object.entries(byStatus)
    .map(([s, n]) => `${n} ${STATUS_LABEL[s]?.toLowerCase() ?? s}`)
    .join(', ');
}

function itemTable(items) {
  const rows = [
    '| Id | Título | Estado | Implementado en | Pendientes |',
    '|---|---|---|---|---|',
  ];
  for (const it of items) {
    const prs = it.prs.length ? it.prs.map((n) => `#${n}`).join(', ') : '—';
    const pending = it.pending.length ? String(it.pending.length) : '—';
    rows.push(`| [${it.id}](${it.path}) | ${cell(it.title)} | ${statusCell(it.status)} | ${prs} | ${pending} |`);
  }
  return rows.join('\n');
}

function sortForTable(items) {
  return [...items].sort((a, b) => compareStatus(a.status, b.status) || a.id.localeCompare(b.id));
}

/** Renders the model as the Markdown of docs/requirements/release-status.md. */
export function renderMarkdown(model) {
  const out = [];
  const allStatuses = Object.keys(model.totals.byStatus);
  out.push('<!-- GENERADO: no editar a mano. Regenerar con `' + REGENERATE + '`. -->');
  out.push('');
  out.push('# Estado de release');
  out.push('');
  out.push(
    `> **Archivo generado, no editar a mano.** Sale del frontmatter (\`status\`) y de la sección "Estado de la implementación" de cada ficha (US, TS, INF, SPIKE y TD) de \`docs/requirements/features/\`. Para cambiar un estado, edita la ficha y regenera con \`${REGENERATE}\`. El CI (docs-lint) falla si este archivo no coincide con lo que genera el script.`,
  );
  out.push('');
  out.push(`Última actualización de una ficha: ${model.lastUpdated ?? '—'}.`);
  out.push('');

  out.push('## Totales por estado');
  out.push('');
  out.push(`| Estado | Fichas |`);
  out.push('|---|---|');
  for (const [s, n] of Object.entries(model.totals.byStatus)) out.push(`| ${statusCell(s)} | ${n} |`);
  out.push(`| **Total** | **${model.totals.total}** |`);
  out.push('');

  out.push('## Por feature');
  out.push('');
  out.push(`| Feature | ${allStatuses.map((s) => STATUS_LABEL[s] ?? s).join(' | ')} | Total |`);
  out.push(`|---|${allStatuses.map(() => '---|').join('')}---|`);
  for (const f of model.features) {
    out.push(`| [${f.name}](#${f.name}) | ${allStatuses.map((s) => f.byStatus[s] ?? 0).join(' | ')} | ${f.total} |`);
  }
  out.push('');
  for (const f of model.features) {
    out.push(`### ${f.name}`);
    out.push('');
    out.push(`${f.total} fichas: ${statusSummary(f.byStatus)}.`);
    out.push('');
    out.push(itemTable(sortForTable(model.items.filter((i) => i.feature === f.name))));
    out.push('');
  }

  out.push('## Por hito');
  out.push('');
  if (model.milestones.length === 0) {
    out.push(
      'Sin agrupado por hito: todavía no hay `docs/requirements/release-plan.md` ni fichas con `milestone` en el frontmatter. Cuando exista el plan, el script asigna cada ficha al primer hito (`## M<n>` o `## Hito M<n>`) que la liste en una fila de tabla.',
    );
    out.push('');
  } else {
    if (model.releasePlan) out.push(`Hitos según [\`${model.releasePlan.replace(/^docs\/requirements\//, '')}\`](${model.releasePlan.replace(/^docs\/requirements\//, '')}) y el campo \`milestone\` de las fichas.`);
    else out.push('Hitos según el campo `milestone` de las fichas.');
    out.push('');
    for (const m of model.milestones) {
      out.push(`### ${m.name}`);
      out.push('');
      out.push(`${m.total} fichas: ${statusSummary(m.byStatus)}.`);
      out.push('');
      out.push(itemTable(sortForTable(model.items.filter((i) => i.milestone === m.name))));
      out.push('');
    }
    const without = model.items.filter((i) => !i.milestone);
    out.push('### Sin hito');
    out.push('');
    out.push(`${without.length} fichas sin hito asignado.`);
    out.push('');
  }

  const partial = model.items.filter((i) => i.status === 'partially-implemented');
  out.push('## Implementadas en parte: lo que falta');
  out.push('');
  if (partial.length === 0) out.push('Ninguna.');
  for (const it of partial) {
    const prs = it.prs.length ? ` — ${it.prs.map((n) => `#${n}`).join(', ')}` : '';
    out.push(`### [${it.id}](${it.path})${prs}`);
    out.push('');
    out.push(cell(it.title));
    out.push('');
    if (it.pending.length === 0) out.push('- *La ficha no lista los pendientes en "Estado de la implementación".*');
    for (const p of it.pending) out.push(`- ${p.replace(/\]\((?!https?:|#)([^)]*)\)/g, (_, link) => `](${rebaseLink(it.path, link)})`)}`);
    out.push('');
  }

  if (model.warnings.length) {
    out.push('## Avisos');
    out.push('');
    for (const w of model.warnings) out.push(`- ${w}`);
    out.push('');
  }
  return out.join('\n').replace(/\n+$/, '\n');
}

// Relative links in a pending bullet point from the card; the report lives in docs/requirements/.
function rebaseLink(cardPath, link) {
  const [target, anchor] = link.split('#');
  if (!target) return link;
  const joined = join(dirname(cardPath), target).split(sep).join('/');
  return anchor === undefined ? joined : `${joined}#${anchor}`;
}

function parseArgs(argv) {
  const opts = { mode: 'write' };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--check') opts.mode = 'check';
    else if (a === '--json') opts.mode = 'json';
    else if (a === '--root' || a === '--plan' || a === '--out') {
      if (argv[i + 1] === undefined) throw new Error(`${a} needs a value`);
      opts[a.slice(2)] = argv[++i];
    } else throw new Error(`unknown argument: ${a}`);
  }
  return opts;
}

export function main(argv = process.argv.slice(2)) {
  const opts = parseArgs(argv);
  const here = dirname(fileURLToPath(import.meta.url));
  const root = resolve(opts.root ?? join(here, '..', '..'));
  const planPath = resolve(opts.plan ?? join(root, 'docs', 'requirements', 'release-plan.md'));
  const outPath = resolve(opts.out ?? join(root, 'docs', 'requirements', 'release-status.md'));
  const model = buildModel({ root, planPath });

  if (opts.mode === 'json') {
    process.stdout.write(JSON.stringify(model, null, 2) + '\n');
    return 0;
  }
  for (const w of model.warnings) console.error(`warning: ${w}`);
  for (const e of model.errors) {
    if (process.env.GITHUB_ACTIONS) console.log(`::error::${e}`);
    console.error(`error: ${e}`);
  }
  const markdown = renderMarkdown(model);
  if (opts.mode === 'check') {
    const current = existsSync(outPath) ? readFileSync(outPath, 'utf8') : null;
    if (model.errors.length) return 1;
    if (current === markdown) {
      console.log(`${relative(root, outPath)} is up to date (${model.totals.total} cards).`);
      return 0;
    }
    const rel = relative(root, outPath);
    const msg = `${rel} is ${current === null ? 'missing' : 'out of date'}. Regenerate it with \`${REGENERATE}\` and commit the result.`;
    if (process.env.GITHUB_ACTIONS) console.log(`::error file=${rel}::${msg}`);
    console.error(msg);
    return 1;
  }
  writeFileSync(outPath, markdown);
  console.log(`Wrote ${relative(root, outPath)} (${model.totals.total} cards).`);
  return model.errors.length ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch (err) {
    console.error(err.message);
    process.exitCode = 2;
  }
}
