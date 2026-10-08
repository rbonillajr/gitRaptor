// Deterministic example samples for the tests and the example report. Times are in
// America/Bogota (UTC-5, no daylight saving), the zone the example report is rendered in.
import { SAMPLE_VERSION } from './lib.mjs';

export const EXAMPLE_TZ = 'America/Bogota';
const OFFSET_H = 5;
export const REPO = 'e8343960-e16a-434e-913f-c3ef0bb49737';

function utc(date, hh, mm) {
  const [y, m, d] = date.split('-').map(Number);
  return Date.UTC(y, m - 1, d, hh + OFFSET_H, mm);
}

const pad = (n) => String(n).padStart(2, '0');

/**
 * One day of samples every `interval` minutes from `from` to `to` (hours, local).
 * `claude(i)` gives the Claude Code sessions present at sample i; `off(i)` says the daemon was off;
 * `events(i)` gives the events since the previous sample as [actor, agent] pairs.
 */
export function buildDay(
  date,
  {
    from = 8,
    to = 17,
    interval = 15,
    claude = () => 3,
    off = () => false,
    events = (i) => (i % 3 === 0 ? [['detected', 'claude-code']] : []),
    rss = () => 46 * 1024 * 1024,
    cpu = (i, idle) => (idle ? 0.4 : 2.1),
    seqStart = 1000,
  } = {},
) {
  const samples = [];
  let seq = seqStart;
  let i = 0;
  for (let t = from * 60; t < to * 60; t += interval, i++) {
    const hh = Math.floor(t / 60);
    const mm = t % 60;
    const ms = utc(date, hh, mm);
    if (off(i)) {
      samples.push({
        v: SAMPLE_VERSION,
        utc_ms: ms,
        date,
        time: `${pad(hh)}:${pad(mm)}`,
        running: false,
        resources: null,
        sessions: null,
        events: [],
        events_truncated: false,
        errors: [],
      });
      continue;
    }
    const evs = events(i).map(([actor, agent], k) => ({
      repo_id: REPO,
      seq: ++seq,
      kind: k % 2 ? 'branch-update' : 'commit',
      worktree: `/work/gitRaptor/w${(seq % 4) + 1}`,
      branch: `feat/x-${seq % 4}`,
      observed_utc_ms: ms - 60_000 + k,
      actor,
      agent,
    }));
    const n = claude(i);
    const list = Array.from({ length: n }, (_, k) => ({
      repo_id: REPO,
      session_id: `${date}-s${k}`,
      worktree: `/work/gitRaptor/w${k + 1}`,
      agent: 'claude-code',
      origin: 'detected',
      state: k === 0 ? 'active' : 'inactive',
      started_utc_ms: utc(date, from, 0),
    }));
    // The latest ended session of a worktree, from an earlier day: it must not count.
    list.push({
      repo_id: REPO,
      session_id: 'old-ended',
      worktree: '/work/gitRaptor/old',
      agent: 'claude-code',
      origin: 'detected',
      state: 'ended',
      started_utc_ms: 0,
    });
    samples.push({
      v: SAMPLE_VERSION,
      utc_ms: ms,
      date,
      time: `${pad(hh)}:${pad(mm)}`,
      running: true,
      resources: {
        pid: 4242,
        cpu_mean_pct: cpu(i, evs.length === 0),
        cpu_peak_pct: 6,
        cpu_window_s: 600,
        cpu_target_pct: 1,
        rss_bytes: rss(i),
        rss_target_bytes: 157286400,
        watch_roots: 10,
      },
      sessions: {
        detection_available: true,
        claude_present: n,
        claude_active: Math.min(n, 1),
        list,
      },
      events: evs,
      events_truncated: false,
      errors: [],
    });
  }
  return samples;
}

/** The three example days of `examples/`: Friday and Monday meet criterion 1, Tuesday is in progress. */
export function exampleDays() {
  return {
    '2026-10-09': buildDay('2026-10-09', {
      claude: (i) => (i >= 8 && i < 20 ? 4 : 2),
      events: (i) => (i % 3 === 0 ? [['detected', 'claude-code'], ['none', null]] : i === 7 ? [['inferred', 'claude-code']] : []),
      seqStart: 1000,
    }),
    '2026-10-12': buildDay('2026-10-12', {
      claude: (i) => (i % 2 ? 3 : 2),
      events: (i) => (i % 4 === 0 ? [['detected', 'claude-code']] : i === 5 ? [['none', null]] : []),
      seqStart: 2000,
    }),
    '2026-10-13': buildDay('2026-10-13', {
      to: 12,
      claude: () => 2,
      off: (i) => i < 2,
      seqStart: 3000,
    }),
  };
}

/** The marks of the example: Friday reviewed, one false positive and a note on Monday. */
export const EXAMPLE_MARKS = {
  events: { 'e8343960:2005': 'human' },
  reviewed: { '2026-10-09': true, '2026-10-12': true },
  notes: { '2026-10-12': ['Un agente hizo `git reset --hard` con trabajo sin commitear; recuperado con `raptor undo` (criterio 3).'] },
};
