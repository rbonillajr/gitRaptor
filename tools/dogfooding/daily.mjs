#!/usr/bin/env node
// Aggregates the dogfooding samples (M1) into a daily report and the running streak.
//
//   node tools/dogfooding/daily.mjs [--date YYYY-MM-DD | --all] [--dir <data>] [--out <reports>]
//   node tools/dogfooding/daily.mjs --mark <event id> human|agent
//   node tools/dogfooding/daily.mjs --review [--date YYYY-MM-DD]
//   node tools/dogfooding/daily.mjs --note "<text>" [--date YYYY-MM-DD]
//
// Writes bitacora/dogfooding/<date>.md and bitacora/dogfooding/racha.md (local, not committed).
// The marks (false positives, reviewed days, notes) live in <data>/marks.json, next to the samples,
// so a regenerated report keeps them.
import { mkdirSync, readdirSync, renameSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import {
  defaultDataDir,
  defaultReportsDir,
  eventId,
  localDate,
  readJson,
  readJsonl,
  unsafeDataDir,
} from './lib.mjs';

// Criterion thresholds of M1 (backlog, "Criterio de salida"). The resource targets come from each
// sample (what `raptor status --resources` reports); these are the fallbacks.
export const RULES = {
  workStart: '08:00',
  workEnd: '17:00',
  daemonOnPct: 90,
  claudeParallel: 3,
  detectionPct: 90,
  minCoveragePct: 50,
  cpuTargetPct: 1,
  rssTargetBytes: 150 * 1024 * 1024,
  defaultIntervalMin: 15,
};

const LIGHT = { green: '🟢', amber: '🟡', red: '🔴', gray: '⚪' };
const DAYS = ['domingo', 'lunes', 'martes', 'miércoles', 'jueves', 'viernes', 'sábado'];

export function weekday(date) {
  const [y, m, d] = date.split('-').map(Number);
  return new Date(y, m - 1, d).getDay();
}

export function isWorkday(date) {
  const w = weekday(date);
  return w >= 1 && w <= 5;
}

/** Nearest-rank percentile of a list of numbers, or null when it is empty. */
export function percentile(values, p) {
  const xs = values.filter((v) => typeof v === 'number' && Number.isFinite(v)).sort((a, b) => a - b);
  if (xs.length === 0) return null;
  const rank = Math.max(1, Math.ceil((p / 100) * xs.length));
  return xs[rank - 1];
}

function median(values) {
  return percentile(values, 50);
}

/** The usual gap between samples, in minutes (the median), or the default. */
function intervalMin(samples) {
  const gaps = [];
  for (let i = 1; i < samples.length; i++) gaps.push((samples[i].utc_ms - samples[i - 1].utc_ms) / 60000);
  if (gaps.length < 2) return RULES.defaultIntervalMin;
  return Math.min(60, Math.max(5, Math.round(median(gaps))));
}

function minutesOf(hhmm) {
  const [h, m] = hhmm.split(':').map(Number);
  return h * 60 + m;
}

/** Everything the report of one day says, computed from its samples and the marks. */
export function aggregateDay(date, samples, marks = {}, rules = RULES) {
  const ordered = [...samples].sort((a, b) => a.utc_ms - b.utc_ms);
  const start = minutesOf(rules.workStart);
  const end = minutesOf(rules.workEnd);
  const work = ordered.filter((s) => {
    const t = minutesOf(s.time);
    return t >= start && t < end;
  });
  const interval = intervalMin(ordered);
  const expected = Math.floor((end - start) / interval);
  const running = ordered.filter((s) => s.running === true);
  const workOn = work.filter((s) => s.running === true);
  const workday = isWorkday(date);

  // Criterion 1: daemon on and Claude Code sessions in parallel, in working hours.
  const daemonOnPct = work.length ? (100 * workOn.length) / work.length : null;
  const claudeMax = Math.max(0, ...workOn.map((s) => s.sessions?.claude_present ?? 0));
  const claudeActiveMax = Math.max(0, ...workOn.map((s) => s.sessions?.claude_active ?? 0));
  const coveragePct = expected ? Math.min(100, (100 * work.length) / expected) : null;
  let c1;
  if (!workday) c1 = { light: 'gray', why: 'No es día laborable: no cuenta para la racha' };
  else if (work.length === 0) c1 = { light: 'gray', why: 'Sin muestras en horario laboral' };
  else {
    const on = daemonOnPct >= rules.daemonOnPct;
    const parallel = claudeMax >= rules.claudeParallel;
    if (!on || !parallel) {
      const why = [];
      if (!on) why.push(`daemon encendido en el ${fmtPct(daemonOnPct)} de las muestras (< ${rules.daemonOnPct} %)`);
      if (!parallel) why.push(`máximo de ${claudeMax} sesiones de Claude Code en paralelo (< ${rules.claudeParallel})`);
      c1 = { light: 'red', why: `No cumple: ${why.join('; ')}` };
    } else if (coveragePct < rules.minCoveragePct) {
      c1 = {
        light: 'amber',
        why: `Cumple, pero con pocas muestras (${work.length} de ${expected} esperadas): revisa si el Mac durmió`,
      };
    } else c1 = { light: 'green', why: 'Cumple' };
  }

  // Criterion 2: who the day's events are attributed to.
  const events = ordered.flatMap((s) => s.events ?? []);
  const byActor = { detected: 0, registered: 0, inferred: 0, none: 0 };
  for (const e of events) byActor[e.actor] = (byActor[e.actor] ?? 0) + 1;
  const claudeEvents = events.filter((e) => e.agent === 'claude-code');
  const claudeDetected = claudeEvents.filter((e) => e.actor === 'detected' || e.actor === 'registered');
  const claudeInferred = claudeEvents.filter((e) => e.actor === 'inferred');
  const detectionPct =
    claudeDetected.length + claudeInferred.length
      ? (100 * claudeDetected.length) / (claudeDetected.length + claudeInferred.length)
      : null;
  const markedHuman = claudeDetected.filter((e) => marks.events?.[eventId(e.repo_id, e.seq)] === 'human');
  const reviewed = Boolean(marks.reviewed?.[date]);
  const sessionIds = new Map();
  for (const s of ordered) {
    for (const x of s.sessions?.list ?? []) {
      // `raptor sessions` also lists the latest ended session of each worktree, often from an
      // earlier day: only the sessions seen present today count.
      if (x.agent === 'claude-code' && x.state !== 'ended') sessionIds.set(`${x.repo_id}:${x.session_id}`, x.origin);
    }
  }
  const sessionsDetected = [...sessionIds.values()].filter((o) => o === 'detected').length;
  let c2;
  if (detectionPct === null) c2 = { light: 'gray', why: 'Sin eventos de Claude Code' };
  else if (markedHuman.length > 0)
    c2 = { light: 'red', why: `${markedHuman.length} evento(s) humanos atribuidos a Claude Code` };
  else if (detectionPct < rules.detectionPct)
    c2 = { light: 'red', why: `Detección del ${fmtPct(detectionPct)} (< ${rules.detectionPct} %)` };
  else if (!reviewed)
    c2 = { light: 'amber', why: 'Detección dentro del objetivo; falta tu revisión de falsos positivos' };
  else c2 = { light: 'green', why: 'Detección dentro del objetivo y día revisado sin falsos positivos' };

  // Criterion 5: CPU at rest and memory of the daemon.
  const withRes = running.filter((s) => s.resources);
  // At rest: no Git event since the previous sample, which covers the CPU window.
  const idle = withRes.filter((s) => (s.events ?? []).length === 0 && !s.events_truncated);
  const cpuTarget = withRes.find((s) => s.resources.cpu_target_pct != null)?.resources.cpu_target_pct ?? rules.cpuTargetPct;
  const rssTarget =
    withRes.find((s) => s.resources.rss_target_bytes != null)?.resources.rss_target_bytes ?? rules.rssTargetBytes;
  const cpuAll = withRes.map((s) => s.resources.cpu_mean_pct);
  const cpuIdle = idle.map((s) => s.resources.cpu_mean_pct);
  const rss = withRes.map((s) => s.resources.rss_bytes);
  const res = {
    samples: withRes.length,
    idleSamples: idle.length,
    cpuAllMedian: median(cpuAll),
    cpuAllP95: percentile(cpuAll, 95),
    cpuIdleMedian: median(cpuIdle),
    cpuIdleP95: percentile(cpuIdle, 95),
    rssMedian: median(rss),
    rssP95: percentile(rss, 95),
    rootsMax: Math.max(0, ...withRes.map((s) => s.resources.watch_roots ?? 0)),
    cpuTarget,
    rssTarget,
  };
  let c5;
  if (withRes.length === 0) c5 = { light: 'gray', why: 'Sin lecturas: el daemon no estuvo encendido' };
  else if (res.rssP95 != null && res.rssP95 >= rssTarget)
    c5 = { light: 'red', why: `RSS p95 ${fmtMiB(res.rssP95)} (≥ ${fmtMiB(rssTarget)})` };
  else if (idle.length === 0)
    c5 = { light: 'amber', why: 'RSS dentro del objetivo; ninguna muestra en reposo para juzgar la CPU' };
  else if (res.cpuIdleMedian >= cpuTarget)
    c5 = { light: 'red', why: `CPU en reposo (mediana) ${fmtPct(res.cpuIdleMedian, 2)} (≥ ${cpuTarget} %)` };
  else c5 = { light: 'green', why: 'CPU en reposo y RSS dentro del objetivo' };

  return {
    date,
    workday,
    samples: ordered.length,
    workSamples: work.length,
    expected,
    interval,
    coveragePct,
    daemonOnPct,
    daemonOnAllPct: ordered.length ? (100 * running.length) / ordered.length : null,
    claudeMax,
    claudeActiveMax,
    byActor,
    totalEvents: events.length,
    truncated: ordered.some((s) => s.events_truncated),
    errors: ordered.flatMap((s) => s.errors ?? []).length,
    claudeEvents: claudeDetected,
    detectionPct,
    claudeDetectedCount: claudeDetected.length,
    claudeInferredCount: claudeInferred.length,
    markedHuman: markedHuman.length,
    reviewed,
    sessions: sessionIds.size,
    sessionsDetected,
    res,
    notes: marks.notes?.[date] ?? [],
    marks: marks.events ?? {},
    c1,
    c2,
    c5,
  };
}

function fmtPct(v, digits = 1) {
  return v == null ? '—' : `${v.toFixed(digits).replace('.', ',')} %`;
}

function fmtMiB(v) {
  return v == null ? '—' : `${(v / 1024 / 1024).toFixed(1).replace('.', ',')} MiB`;
}

function share(n, total) {
  return total ? fmtPct((100 * n) / total) : '—';
}

function timeOf(ms) {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

function cell(text) {
  return String(text ?? '').replace(/\|/g, '\\|').replace(/[\r\n]+/g, ' ');
}

/** The Markdown of one day. */
export function renderDay(m) {
  const L = (c) => `${LIGHT[c.light]} ${c.why}`;
  const out = [];
  out.push(`# Dogfooding ${m.date} (${DAYS[weekday(m.date)]})`, '');
  out.push(
    '> Generado por `tools/dogfooding/daily.mjs` a partir de las muestras del día (por defecto en `~/.gitraptor-dogfooding/`). ' +
      'No lo edites a mano: se regenera con cada muestra. Las marcas y notas se añaden con `daily.mjs --mark`, `--review` y `--note`.',
    '',
  );
  out.push('## Semáforo de M1', '');
  out.push('| Criterio | Estado |', '|---|---|');
  out.push(`| 1. Uso real | ${cell(L(m.c1))} |`);
  out.push(`| 2. Detección | ${cell(L(m.c2))} |`);
  out.push(`| 5. Recursos | ${cell(L(m.c5))} |`);
  out.push('');
  out.push('## Uso', '');
  out.push(`- Muestras: ${m.samples} en el día; ${m.workSamples} de ${m.expected} esperadas en horario laboral (${RULES.workStart} a ${RULES.workEnd}, cada ${m.interval} min).`);
  out.push(`- Daemon encendido: ${fmtPct(m.daemonOnPct)} de las muestras en horario laboral (${fmtPct(m.daemonOnAllPct)} en todo el día).`);
  out.push(`- Máximo de sesiones de Claude Code en paralelo: ${m.claudeMax} presentes (activas o en espera); ${m.claudeActiveMax} activas a la vez.`);
  out.push(`- Sesiones de Claude Code vistas en el día: ${m.sessions} (${m.sessionsDetected} detectadas, ${m.sessions - m.sessionsDetected} registradas).`);
  if (m.errors) out.push(`- ⚠️ ${m.errors} error(es) al consultar la CLI: mira el \`.jsonl\` del día.`);
  out.push('');
  out.push('## Eventos por actor', '');
  out.push('| Actor | Eventos | Proporción |', '|---|---|---|');
  const rows = [
    ['Agente detectado', m.byActor.detected],
    ['Agente registrado', m.byActor.registered],
    ['Inferido (sin atribución, con sesión sugerida)', m.byActor.inferred],
    ['Sin agente', m.byActor.none],
  ];
  for (const [name, n] of rows) out.push(`| ${name} | ${n} | ${share(n, m.totalEvents)} |`);
  out.push(`| **Total** | **${m.totalEvents}** | |`, '');
  out.push(
    `Detección de Claude Code: ${fmtPct(m.detectionPct)} ` +
      `(${m.claudeDetectedCount} eventos atribuidos frente a ${m.claudeInferredCount} solo inferidos). ` +
      'Es una aproximación: el motor no ve las sesiones que no detectó y que tampoco dejaron un evento inferido.',
  );
  if (m.truncated) out.push('', '⚠️ En alguna muestra pudo faltar algún evento (la página no llegó al cursor).');
  out.push('');
  out.push('## Recursos del daemon', '');
  out.push('| Lectura | Mediana | p95 | Objetivo |', '|---|---|---|---|');
  out.push(`| CPU en reposo (${m.res.idleSamples} muestras sin eventos) | ${fmtPct(m.res.cpuIdleMedian, 2)} | ${fmtPct(m.res.cpuIdleP95, 2)} | < ${m.res.cpuTarget} % |`);
  out.push(`| CPU, todas las muestras (${m.res.samples}) | ${fmtPct(m.res.cpuAllMedian, 2)} | ${fmtPct(m.res.cpuAllP95, 2)} | — |`);
  out.push(`| RSS | ${fmtMiB(m.res.rssMedian)} | ${fmtMiB(m.res.rssP95)} | < ${fmtMiB(m.res.rssTarget)} |`);
  out.push('');
  out.push(
    `Raíces observadas (máximo): ${m.res.rootsMax}. El objetivo de M1 se fija con 10 worktrees. ` +
      '"En reposo" es una aproximación: una muestra sin eventos Git desde la anterior (la ventana de CPU es de unos 10 minutos).',
  );
  out.push('');
  out.push('## Falsos positivos (criterio 2: 0 trabajo humano atribuido a Claude Code)', '');
  out.push(
    'El motor no puede saber si un evento atribuido a Claude Code lo hiciste tú. Revisa la lista y marca los tuyos:',
    '',
    '```sh',
    'node tools/dogfooding/daily.mjs --mark <id> human   # ese evento lo hiciste tú',
    'node tools/dogfooding/daily.mjs --mark <id> agent   # deshace la marca',
    `node tools/dogfooding/daily.mjs --review --date ${m.date}   # revisé el día`,
    '```',
    '',
  );
  out.push(`Estado: ${m.reviewed ? 'revisado' : 'sin revisar'}; ${m.markedHuman} marcado(s) como humanos.`, '');
  if (m.claudeEvents.length) {
    const byWorktree = new Map();
    for (const e of m.claudeEvents) {
      if (!byWorktree.has(e.worktree)) byWorktree.set(e.worktree, []);
      byWorktree.get(e.worktree).push(e);
    }
    out.push(`<details><summary>${m.claudeEvents.length} eventos atribuidos a Claude Code</summary>`, '');
    for (const [wt, list] of byWorktree) {
      out.push(`**${cell(wt)}**`, '', '| Id | Hora | Tipo | Rama | Marca |', '|---|---|---|---|---|');
      for (const e of list) {
        const id = eventId(e.repo_id, e.seq);
        const mark = m.marks[id] === 'human' ? '❌ humano' : '';
        out.push(`| \`${id}\` | ${timeOf(e.observed_utc_ms)} | ${cell(e.kind)} | ${cell(e.branch ?? '')} | ${mark} |`);
      }
      out.push('');
    }
    out.push('</details>', '');
  }
  out.push('## Incidentes y notas', '');
  if (m.notes.length) for (const n of m.notes) out.push(`- ${n}`);
  else out.push('Ninguna. Añade una con `node tools/dogfooding/daily.mjs --note "<texto>"` (por ejemplo, un `raptor undo` real: criterio 3).');
  out.push('');
  return out.join('\n');
}

/** Every weekday between two dates, both included. */
function dateRange(from, to) {
  const out = [];
  const [y, m, d] = from.split('-').map(Number);
  const cur = new Date(y, m - 1, d);
  for (;;) {
    const s = localDate(cur.getTime());
    out.push(s);
    if (s >= to) break;
    cur.setDate(cur.getDate() + 1);
  }
  return out;
}

/** A day counts for the streak when criterion 1 is green or amber. */
function counts(day) {
  return day && (day.c1.light === 'green' || day.c1.light === 'amber');
}

/**
 * The streak of criterion 1: consecutive workdays that meet it, up to the last day with data.
 * A workday with no data breaks it. `today`, while it does not meet the criterion yet, is "in
 * progress" and does not break it.
 */
export function aggregateStreak(models, today) {
  const byDate = new Map(models.map((m) => [m.date, m]));
  const dates = [...byDate.keys()].sort();
  if (dates.length === 0) return { days: [], current: 0, best: 0, totals: null };
  const days = dateRange(dates[0], dates[dates.length - 1])
    .filter(isWorkday)
    .map((date) => ({ date, model: byDate.get(date) ?? null }));
  let best = 0;
  let run = 0;
  for (const d of days) {
    if (counts(d.model)) best = Math.max(best, ++run);
    else if (d.date !== today) run = 0;
  }
  let current = 0;
  for (let i = days.length - 1; i >= 0; i--) {
    const d = days[i];
    if (counts(d.model)) current++;
    else if (d.date === today) continue;
    else break;
  }
  const totals = { detected: 0, inferred: 0, sessions: 0, markedHuman: 0, reviewed: 0 };
  for (const m of models) {
    totals.detected += m.claudeDetectedCount;
    totals.inferred += m.claudeInferredCount;
    totals.sessions += m.sessions;
    totals.markedHuman += m.markedHuman;
    if (m.reviewed) totals.reviewed++;
  }
  return { days, current, best, totals, allDays: models.length };
}

/** The Markdown of the streak and the running totals. */
export function renderStreak(s, today) {
  const out = ['# Racha de dogfooding (M1)', ''];
  out.push(
    '> Generado por `tools/dogfooding/daily.mjs`. Criterio 1 de M1: 10 días laborables seguidos con el daemon encendido y al menos 3 sesiones de Claude Code en paralelo. Un día laborable sin datos rompe la racha; el día en curso no.',
    '',
  );
  const goal = 10;
  out.push(`- **Racha actual: ${s.current} de ${goal} días laborables.** ${s.current >= goal ? '🟢 Criterio 1 cumplido.' : ''}`.trimEnd());
  out.push(`- Mejor racha: ${s.best}.`);
  if (s.totals) {
    const t = s.totals;
    const det = t.detected + t.inferred ? (100 * t.detected) / (t.detected + t.inferred) : null;
    out.push(
      `- Acumulado (criterio 2): ${t.sessions} sesiones de Claude Code vistas (objetivo: al menos 50 en 2 semanas); ` +
        `detección ${fmtPct(det)}; ${t.markedHuman} falso(s) positivo(s) marcados; ${t.reviewed} de ${s.allDays} días revisados.`,
    );
  }
  out.push('');
  out.push('| Fecha | Día | C1 Uso real | Daemon | Claude en paralelo | C2 Detección | C5 Recursos | CPU reposo | RSS p95 |');
  out.push('|---|---|---|---|---|---|---|---|---|');
  for (const { date, model: m } of [...s.days].reverse()) {
    const day = DAYS[weekday(date)];
    if (!m) {
      out.push(`| ${date} | ${day} | ${LIGHT.gray} sin datos | — | — | — | — | — | — |`);
      continue;
    }
    const c1 = date === today && !counts(m) ? '⏳ en curso' : LIGHT[m.c1.light];
    out.push(
      `| [${date}](${date}.md) | ${day} | ${c1} | ${fmtPct(m.daemonOnPct)} | ${m.claudeMax} | ${LIGHT[m.c2.light]} ${fmtPct(m.detectionPct)} | ` +
        `${LIGHT[m.c5.light]} | ${fmtPct(m.res.cpuIdleMedian, 2)} | ${fmtMiB(m.res.rssP95)} |`,
    );
  }
  out.push('');
  return out.join('\n');
}

function atomicWrite(path, text) {
  const tmp = `${path}.tmp`;
  writeFileSync(tmp, text);
  renameSync(tmp, path);
}

/** Every date with a sample file in the data directory. */
export function sampleDates(dataDir) {
  let names = [];
  try {
    names = readdirSync(dataDir);
  } catch {
    return [];
  }
  return names
    .filter((n) => /^\d{4}-\d{2}-\d{2}\.jsonl$/.test(n))
    .map((n) => n.slice(0, 10))
    .sort();
}

/** Writes the report of `date` (or of every day with `all`) and the streak. */
export function writeDailyReports({ dataDir, outDir, date = null, all = false, today = localDate(Date.now()) }) {
  const marks = readJson(join(dataDir, 'marks.json'), {});
  const models = sampleDates(dataDir).map((d) => aggregateDay(d, readJsonl(join(dataDir, `${d}.jsonl`)), marks));
  mkdirSync(outDir, { recursive: true });
  const written = [];
  for (const m of models) {
    if (all || m.date === (date ?? today)) {
      atomicWrite(join(outDir, `${m.date}.md`), renderDay(m));
      written.push(`${m.date}.md`);
    }
  }
  atomicWrite(join(outDir, 'racha.md'), renderStreak(aggregateStreak(models, today), today));
  written.push('racha.md');
  return written;
}

export function updateMarks(marks, { mark, value, review, note, date }) {
  const next = { events: {}, reviewed: {}, notes: {}, ...marks };
  if (mark) {
    if (!/^[0-9a-f-]{1,8}:\d+$/i.test(mark)) throw new Error(`not an event id: ${mark} (expected <repo>:<seq>)`);
    if (value === 'human') next.events = { ...next.events, [mark]: 'human' };
    else if (value === 'agent') {
      next.events = { ...next.events };
      delete next.events[mark];
    } else throw new Error(`--mark takes human or agent, not ${value}`);
  }
  if (review) next.reviewed = { ...next.reviewed, [date]: true };
  if (note) next.notes = { ...next.notes, [date]: [...(next.notes[date] ?? []), note] };
  return next;
}

function main() {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      date: { type: 'string' },
      all: { type: 'boolean', default: false },
      dir: { type: 'string', default: defaultDataDir() },
      out: { type: 'string', default: defaultReportsDir() },
      mark: { type: 'string' },
      review: { type: 'boolean', default: false },
      note: { type: 'string' },
    },
  });
  const unsafe = unsafeDataDir(values.dir);
  if (unsafe) {
    console.error(`daily: refusing the data directory: ${unsafe}`);
    process.exit(2);
  }
  const today = localDate(Date.now());
  const date = values.date ?? today;
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) {
    console.error(`daily: --date takes YYYY-MM-DD, not ${date}`);
    process.exit(2);
  }
  if (values.mark || values.review || values.note) {
    const path = join(values.dir, 'marks.json');
    let next;
    try {
      next = updateMarks(readJson(path, {}), { ...values, value: positionals[0], date });
    } catch (err) {
      console.error(`daily: ${err.message}`);
      process.exit(2);
    }
    mkdirSync(values.dir, { recursive: true });
    atomicWrite(path, `${JSON.stringify(next, null, 2)}\n`);
  }
  const written = writeDailyReports({ dataDir: values.dir, outDir: values.out, date, all: values.all, today });
  console.log(`daily: wrote ${written.join(', ')} in ${values.out}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
