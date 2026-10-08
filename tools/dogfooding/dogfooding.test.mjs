// Tests for the dogfooding log on example samples in temporary directories, never this repo
// nor the GitRaptor profile (NFR-01).
//   node --test tools/dogfooding/dogfooding.test.mjs
// UPDATE_EXAMPLES=1 node --test tools/dogfooding/dogfooding.test.mjs rewrites tools/dogfooding/examples/.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { aggregateDay, aggregateStreak, percentile, updateMarks } from './daily.mjs';
import { EXAMPLE_MARKS, EXAMPLE_TZ, REPO, buildDay, exampleDays } from './fixtures.mjs';
import { actorClass, unsafeDataDir } from './lib.mjs';
import { newEvents, takeSample } from './sample.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
let tmp;
before(() => {
  tmp = mkdtempSync(join(tmpdir(), 'gitraptor-dogfooding-'));
});
after(() => rmSync(tmp, { recursive: true, force: true }));

function writeSamples(dir, days) {
  mkdirSync(dir, { recursive: true });
  for (const [date, samples] of Object.entries(days)) {
    writeFileSync(join(dir, `${date}.jsonl`), samples.map((s) => JSON.stringify(s)).join('\n') + '\n');
  }
}

function node(script, args, env = {}) {
  const r = spawnSync(process.execPath, [join(HERE, script), ...args], {
    encoding: 'utf8',
    env: { ...process.env, TZ: EXAMPLE_TZ, ...env },
  });
  return r;
}

describe('sample', () => {
  test('the actor class of an event', () => {
    assert.equal(actorClass({ actor: { actor: 'agent', kind: 'claude-code', origin: 'detected' } }), 'detected');
    assert.equal(actorClass({ actor: { actor: 'agent', kind: 'other', origin: 'registered' } }), 'registered');
    assert.equal(actorClass({ actor: 'unattributed', inferred: { kind: 'claude-code', session_id: 's' } }), 'inferred');
    assert.equal(actorClass({ actor: 'unattributed', inferred: null }), 'none');
  });

  test('only the events after the cursor, and the cursor moves', () => {
    const ev = (seq, ms = 10) => ({ repo_id: 'r1', seq, observed_utc_ms: ms, actor: 'unattributed', kind: 'commit' });
    const got = newEvents([ev(5), ev(6), ev(7)], { r1: 5 }, 0);
    assert.deepEqual(got.fresh.map((e) => e.seq), [6, 7]);
    assert.deepEqual(got.cursor, { r1: 7 });
    assert.equal(got.truncated, false);
  });

  test('without a cursor, only the events since the start of the day', () => {
    const ev = (seq, ms) => ({ repo_id: 'r1', seq, observed_utc_ms: ms, actor: 'unattributed', kind: 'commit' });
    const got = newEvents([ev(1, 50), ev(2, 150)], {}, 100);
    assert.deepEqual(got.fresh.map((e) => e.seq), [2]);
  });

  test('a page that does not reach the cursor is flagged', () => {
    const ev = (seq) => ({ repo_id: 'r1', seq, observed_utc_ms: 1, actor: 'unattributed', kind: 'commit' });
    assert.equal(newEvents([ev(20), ev(21)], { r1: 5 }, 0).truncated, true);
  });

  test('with the daemon off it asks nothing that would start it', () => {
    const calls = [];
    const run = (args) => {
      calls.push(args.join(' '));
      return { running: false, engine: null };
    };
    const { sample } = takeSample({ run, nowMs: Date.UTC(2026, 9, 9, 15), state: {} });
    assert.deepEqual(calls, ['status --resources --json']);
    assert.equal(sample.running, false);
    assert.equal(sample.sessions, null);
  });

  test('with the daemon on it reads sessions and new events, and falls back to --all', () => {
    const calls = [];
    const evt = (seq) => ({
      repo_id: REPO,
      seq,
      observed_utc_ms: Date.UTC(2026, 9, 9, 15),
      kind: 'commit',
      worktree: '/w',
      actor: { actor: 'agent', kind: 'claude-code', origin: 'detected' },
      inferred: null,
    });
    const run = (args) => {
      calls.push(args.join(' '));
      if (args[0] === 'status') return { running: true, engine: { pid: 1, cpu: { mean_pct: 0.2 }, rss_bytes: { value: 9 } } };
      if (args[0] === 'sessions') return { detection_available: true, sessions: [] };
      if (args.includes('--all')) return [evt(11), evt(12), evt(13)];
      return [evt(13)];
    };
    const { sample, state } = takeSample({ run, nowMs: Date.UTC(2026, 9, 9, 16), state: { last_seq: { [REPO]: 10 } } });
    assert.deepEqual(calls, ['status --resources --json', 'sessions --json', 'events --json --limit 200', 'events --json --all']);
    assert.deepEqual(sample.events.map((e) => e.seq), [11, 12, 13]);
    assert.equal(sample.resources.cpu_mean_pct, 0.2);
    assert.deepEqual(state.last_seq, { [REPO]: 13 });
  });

  test('end to end with a fake raptor: writes only to the data and reports folders', () => {
    const bin = join(tmp, 'bin');
    mkdirSync(bin, { recursive: true });
    const fake = join(bin, 'raptor');
    writeFileSync(
      fake,
      `#!${process.execPath}\n` +
        `const a = process.argv.slice(2).join(' ');\n` +
        `if (a.startsWith('status')) console.log(JSON.stringify({ running: true, engine: { pid: 7, cpu: { mean_pct: 0.3, target_pct: 1 }, rss_bytes: { value: 1000, target: 157286400 }, watches: { roots: 10 } } }));\n` +
        `else if (a.startsWith('sessions')) console.log(JSON.stringify({ detection_available: true, sessions: [] }));\n` +
        `else console.log('[]');\n`,
    );
    chmodSync(fake, 0o755);
    const data = join(tmp, 'e2e-data');
    const out = join(tmp, 'e2e-out');
    const r = node('sample.mjs', ['--raptor', fake, '--dir', data, '--out', out]);
    assert.equal(r.status, 0, r.stderr);
    const files = readdirSync(data).sort();
    assert.ok(files.some((f) => /^\d{4}-\d{2}-\d{2}\.jsonl$/.test(f)));
    assert.ok(files.includes('state.json'));
    assert.ok(readdirSync(out).includes('racha.md'));
  });

  test('refuses a data folder inside a repo or the profile', () => {
    const repo = join(tmp, 'a-repo');
    mkdirSync(join(repo, '.git'), { recursive: true });
    assert.match(unsafeDataDir(join(repo, 'samples')), /inside the repo/);
    assert.match(unsafeDataDir(join(tmp, 'Library', 'Application Support', 'gitraptor', 'x')), /profile/);
    assert.equal(unsafeDataDir(join(tmp, '.gitraptor-dogfooding')), null);
    const r = node('sample.mjs', ['--raptor', '/bin/false', '--dir', join(repo, 'samples')]);
    assert.equal(r.status, 2);
    assert.equal(existsSync(join(repo, 'samples')), false);
  });
});

describe('daily', () => {
  test('percentile is nearest-rank', () => {
    assert.equal(percentile([5, 1, 3, 2, 4], 50), 3);
    assert.equal(percentile([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 95), 10);
    assert.equal(percentile([], 50), null);
  });

  test('a good workday is green on criterion 1', () => {
    const m = aggregateDay('2026-10-09', buildDay('2026-10-09'));
    assert.equal(m.c1.light, 'green');
    assert.equal(m.daemonOnPct, 100);
    assert.equal(m.claudeMax, 3);
    assert.equal(m.workSamples, 36);
    assert.equal(m.expected, 36);
  });

  test('fewer than 3 sessions or the daemon off is red', () => {
    assert.equal(aggregateDay('2026-10-09', buildDay('2026-10-09', { claude: () => 2 })).c1.light, 'red');
    const off = aggregateDay('2026-10-09', buildDay('2026-10-09', { off: (i) => i % 5 === 0 }));
    assert.equal(off.c1.light, 'red');
    assert.match(off.c1.why, /daemon encendido/);
  });

  test('a weekend day does not count, few samples are amber', () => {
    assert.equal(aggregateDay('2026-10-10', buildDay('2026-10-10')).c1.light, 'gray');
    assert.equal(aggregateDay('2026-10-09', buildDay('2026-10-09', { to: 11 })).c1.light, 'amber');
  });

  test('sessions ended on an earlier day are not counted', () => {
    const m = aggregateDay('2026-10-09', buildDay('2026-10-09', { claude: () => 3 }));
    assert.equal(m.sessions, 3);
  });

  test('criterion 2: amber until reviewed, red with a false positive or low detection', () => {
    const day = buildDay('2026-10-09', { seqStart: 100 });
    assert.equal(aggregateDay('2026-10-09', day).c2.light, 'amber');
    assert.equal(aggregateDay('2026-10-09', day, { reviewed: { '2026-10-09': true } }).c2.light, 'green');
    const human = aggregateDay('2026-10-09', day, { events: { 'e8343960:101': 'human' }, reviewed: { '2026-10-09': true } });
    assert.equal(human.c2.light, 'red');
    assert.equal(human.markedHuman, 1);
    const low = buildDay('2026-10-09', { events: (i) => (i % 2 ? [['inferred', 'claude-code']] : [['detected', 'claude-code']]) });
    assert.equal(aggregateDay('2026-10-09', low).c2.light, 'red');
  });

  test('criterion 5 judges idle CPU and RSS against the targets', () => {
    assert.equal(aggregateDay('2026-10-09', buildDay('2026-10-09')).c5.light, 'green');
    const fat = aggregateDay('2026-10-09', buildDay('2026-10-09', { rss: () => 200 * 1024 * 1024 }));
    assert.equal(fat.c5.light, 'red');
    const hot = aggregateDay('2026-10-09', buildDay('2026-10-09', { cpu: () => 1.5 }));
    assert.equal(hot.c5.light, 'red');
    const busy = aggregateDay('2026-10-09', buildDay('2026-10-09', { events: () => [['none', null]] }));
    assert.equal(busy.c5.light, 'amber');
    assert.equal(busy.res.idleSamples, 0);
  });

  test('the streak counts workdays, skips weekends, a missing workday breaks it, today is in progress', () => {
    const good = (d) => aggregateDay(d, buildDay(d));
    const bad = (d) => aggregateDay(d, buildDay(d, { claude: () => 1 }));
    // Thu 8, Fri 9, (weekend), Mon 12, Tue 13 in progress.
    let s = aggregateStreak([good('2026-10-08'), good('2026-10-09'), good('2026-10-12'), bad('2026-10-13')], '2026-10-13');
    assert.equal(s.current, 3);
    // Tuesday finished and failed: the streak is broken.
    s = aggregateStreak([good('2026-10-08'), good('2026-10-09'), good('2026-10-12'), bad('2026-10-13')], '2026-10-14');
    assert.equal(s.current, 0);
    assert.equal(s.best, 3);
    // No data on Friday breaks it.
    s = aggregateStreak([good('2026-10-08'), good('2026-10-12')], '2026-10-12');
    assert.equal(s.current, 1);
  });

  test('marks: human, undo, review and notes; a bad id is refused', () => {
    let m = updateMarks({}, { mark: 'e8343960:12', value: 'human', date: '2026-10-09' });
    assert.equal(m.events['e8343960:12'], 'human');
    m = updateMarks(m, { mark: 'e8343960:12', value: 'agent', date: '2026-10-09' });
    assert.equal(m.events['e8343960:12'], undefined);
    m = updateMarks(m, { review: true, note: 'undo real', date: '2026-10-09' });
    assert.equal(m.reviewed['2026-10-09'], true);
    assert.deepEqual(m.notes['2026-10-09'], ['undo real']);
    assert.throws(() => updateMarks({}, { mark: '../x', value: 'human' }), /not an event id/);
    assert.throws(() => updateMarks({}, { mark: 'abc:1', value: 'robot' }), /human or agent/);
  });

  test('the CLI marks an event and the report shows it', () => {
    const data = join(tmp, 'cli-data');
    const out = join(tmp, 'cli-out');
    writeSamples(data, { '2026-10-09': buildDay('2026-10-09', { seqStart: 100 }) });
    let r = node('daily.mjs', ['--dir', data, '--out', out, '--date', '2026-10-09', '--mark', 'e8343960:101', 'human']);
    assert.equal(r.status, 0, r.stderr);
    const report = readFileSync(join(out, '2026-10-09.md'), 'utf8');
    assert.match(report, /`e8343960:101` .*❌ humano/);
    assert.match(report, /🔴 1 evento\(s\) humanos/);
    r = node('daily.mjs', ['--dir', data, '--out', out, '--mark', 'nope', 'human']);
    assert.equal(r.status, 2);
  });

  test('the example report in examples/ is what the example samples produce', () => {
    const examples = join(HERE, 'examples');
    const gen = join(tmp, 'ex-data');
    const target = join(tmp, 'ex-out');
    writeSamples(gen, exampleDays());
    writeFileSync(join(gen, 'marks.json'), `${JSON.stringify(EXAMPLE_MARKS, null, 2)}\n`);
    // Rendered in the example's zone and with its own "today" (Tuesday, in progress).
    const script = `
      import { writeDailyReports } from ${JSON.stringify(join(HERE, 'daily.mjs'))};
      writeDailyReports({ dataDir: ${JSON.stringify(gen)}, outDir: ${JSON.stringify(target)}, all: true, today: '2026-10-13' });`;
    const r = spawnSync(process.execPath, ['--input-type=module', '-e', script], {
      encoding: 'utf8',
      env: { ...process.env, TZ: EXAMPLE_TZ },
    });
    assert.equal(r.status, 0, r.stderr);
    if (process.env.UPDATE_EXAMPLES) writeSamples(join(examples, 'samples'), exampleDays());
    for (const name of ['2026-10-09.md', '2026-10-12.md', '2026-10-13.md', 'racha.md']) {
      const got = readFileSync(join(target, name), 'utf8');
      if (process.env.UPDATE_EXAMPLES) writeFileSync(join(examples, name), got);
      else assert.equal(got, readFileSync(join(examples, name), 'utf8'), `${name} is stale: run with UPDATE_EXAMPLES=1`);
    }
  });
});
