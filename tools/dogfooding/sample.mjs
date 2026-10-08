#!/usr/bin/env node
// Takes one sample of the dogfooding log (M1) and refreshes today's report.
//
//   node tools/dogfooding/sample.mjs [--raptor <path>] [--dir <data dir>] [--out <reports dir>]
//                                    [--no-report]
//
// Uses only the public CLI with --json, never a reserved command, and writes only to the data
// directory (default ~/.gitraptor-dogfooding, outside every repo and the GitRaptor profile, NFR-01)
// and to the reports directory (bitacora/dogfooding/, local and not committed).
//
// `raptor daemon status` and `raptor sessions` / `raptor events` start the engine when it is not
// running, which would hide whether the daemon was on. So the sample asks
// `raptor status --resources --json` first (it never starts the engine) and asks for sessions and
// events only when the daemon is already running.
import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdirSync, renameSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import {
  SAMPLE_VERSION,
  actorClass,
  actorKind,
  defaultDataDir,
  defaultReportsDir,
  localDate,
  localTime,
  readJson,
  unsafeDataDir,
} from './lib.mjs';
import { writeDailyReports } from './daily.mjs';

const TIMEOUT_MS = 30_000;
const EVENT_PAGE = 200;

/** Runs `raptor <args>` with a fixed argv (no shell) and parses its JSON output. */
export function runRaptor(raptor, args) {
  const out = execFileSync(raptor, args, {
    encoding: 'utf8',
    timeout: TIMEOUT_MS,
    stdio: ['ignore', 'pipe', 'pipe'],
    maxBuffer: 64 * 1024 * 1024,
  });
  return JSON.parse(out);
}

/** The readings of `raptor status --resources --json` the log keeps. */
export function resourcesOf(json) {
  const e = json?.engine;
  if (!e) return null;
  return {
    pid: e.pid ?? null,
    cpu_mean_pct: e.cpu?.mean_pct ?? null,
    cpu_peak_pct: e.cpu?.peak_pct ?? null,
    cpu_window_s: e.cpu?.window_s ?? null,
    cpu_target_pct: e.cpu?.target_pct ?? null,
    rss_bytes: e.rss_bytes?.value ?? null,
    rss_target_bytes: e.rss_bytes?.target ?? null,
    watch_roots: e.watches?.roots ?? null,
  };
}

/** The sessions of `raptor sessions --json`, reduced to what the report counts. */
export function sessionsOf(json) {
  const list = (json?.sessions ?? []).map((s) => ({
    repo_id: s.repo_id,
    session_id: s.session_id,
    worktree: s.worktree,
    agent: s.agent,
    origin: s.origin,
    state: s.state,
    started_utc_ms: s.started_utc_ms,
  }));
  const claude = list.filter((s) => s.agent === 'claude-code');
  return {
    detection_available: json?.detection_available ?? null,
    claude_present: claude.filter((s) => s.state !== 'ended').length,
    claude_active: claude.filter((s) => s.state === 'active').length,
    list,
  };
}

/**
 * The events of `raptor events --json` that are newer than the cursor (`last_seq` per repo).
 * With no cursor for a repo, only the events observed since `sinceMs`. Returns the new events,
 * the advanced cursor and whether some events may be missing (the page did not reach the cursor).
 */
export function newEvents(events, lastSeq, sinceMs) {
  const cursor = { ...lastSeq };
  const fresh = [];
  const oldestByRepo = {};
  for (const e of events) {
    const known = lastSeq[e.repo_id];
    oldestByRepo[e.repo_id] = Math.min(oldestByRepo[e.repo_id] ?? Infinity, e.seq);
    const isNew = known === undefined ? e.observed_utc_ms >= sinceMs : e.seq > known;
    if (isNew) {
      fresh.push({
        repo_id: e.repo_id,
        seq: e.seq,
        kind: e.kind,
        worktree: e.worktree,
        branch: e.branch ?? null,
        observed_utc_ms: e.observed_utc_ms,
        actor: actorClass(e),
        agent: actorKind(e),
      });
    }
    cursor[e.repo_id] = Math.max(cursor[e.repo_id] ?? -Infinity, e.seq);
  }
  const truncated = Object.entries(oldestByRepo).some(
    ([repo, oldest]) => lastSeq[repo] !== undefined && oldest > lastSeq[repo] + 1,
  );
  fresh.sort((a, b) => a.observed_utc_ms - b.observed_utc_ms || a.seq - b.seq);
  return { fresh, cursor, truncated };
}

/** Takes one sample. `run(args)` is `raptor` with those arguments, parsed as JSON. */
export function takeSample({ run, nowMs, state }) {
  const sample = {
    v: SAMPLE_VERSION,
    utc_ms: nowMs,
    date: localDate(nowMs),
    time: localTime(nowMs),
    running: null,
    resources: null,
    sessions: null,
    events: [],
    events_truncated: false,
    errors: [],
  };
  let nextState = state;
  try {
    const res = run(['status', '--resources', '--json']);
    sample.running = res.running === true;
    sample.resources = resourcesOf(res);
  } catch (err) {
    sample.errors.push({ cmd: 'status --resources', message: String(err.message).slice(0, 300) });
    return { sample, state: nextState };
  }
  if (!sample.running) return { sample, state: nextState };

  try {
    sample.sessions = sessionsOf(run(['sessions', '--json']));
  } catch (err) {
    sample.errors.push({ cmd: 'sessions', message: String(err.message).slice(0, 300) });
  }
  try {
    const startOfDay = new Date(nowMs);
    startOfDay.setHours(0, 0, 0, 0);
    const lastSeq = state.last_seq ?? {};
    let page = run(['events', '--json', '--limit', String(EVENT_PAGE)]);
    let got = newEvents(page, lastSeq, startOfDay.getTime());
    if (got.truncated) {
      page = run(['events', '--json', '--all']);
      got = newEvents(page, lastSeq, startOfDay.getTime());
    }
    sample.events = got.fresh;
    sample.events_truncated = got.truncated;
    nextState = { ...state, last_seq: got.cursor };
  } catch (err) {
    sample.errors.push({ cmd: 'events', message: String(err.message).slice(0, 300) });
  }
  return { sample, state: nextState };
}

function main() {
  const { values } = parseArgs({
    options: {
      raptor: { type: 'string', default: process.env.GITRAPTOR_DOGFOODING_RAPTOR || 'raptor' },
      dir: { type: 'string', default: defaultDataDir() },
      out: { type: 'string', default: defaultReportsDir() },
      'no-report': { type: 'boolean', default: false },
    },
  });
  const unsafe = unsafeDataDir(values.dir);
  if (unsafe) {
    console.error(`sample: refusing the data directory: ${unsafe}`);
    process.exit(2);
  }
  mkdirSync(values.dir, { recursive: true });
  const statePath = join(values.dir, 'state.json');
  const state = readJson(statePath, {});
  const { sample, state: next } = takeSample({
    run: (args) => runRaptor(values.raptor, args),
    nowMs: Date.now(),
    state,
  });
  appendFileSync(join(values.dir, `${sample.date}.jsonl`), `${JSON.stringify(sample)}\n`);
  const tmp = `${statePath}.tmp`;
  writeFileSync(tmp, `${JSON.stringify(next, null, 2)}\n`);
  renameSync(tmp, statePath);
  console.log(
    `sample: ${sample.date} ${sample.time} running=${sample.running} ` +
      `claude=${sample.sessions?.claude_present ?? '-'} events=${sample.events.length}` +
      (sample.errors.length ? ` errors=${sample.errors.length}` : ''),
  );
  if (!values['no-report']) {
    writeDailyReports({ dataDir: values.dir, outDir: values.out, date: sample.date });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
