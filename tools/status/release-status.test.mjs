// Tests for release-status.mjs on example cards in a temporary directory, never this repo.
//   node --test tools/status/release-status.test.mjs
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  buildModel,
  parseFrontmatter,
  parseImplementation,
  parseReleasePlan,
  renderMarkdown,
} from './release-status.mjs';

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), 'release-status.mjs');

function card({ id, title, type, status, updated = '2026-10-01', extra = '', body = '' }) {
  return `---\nid: ${id}\ntitle: "${title}"\ntype: ${type}\nstatus: ${status}\nupdated: ${updated}\n${extra}related:\n  stories: [US-X-001]\ntags: [a, b]\n---\n\n# ${id}\n\n${body}`;
}

const PARTIAL_BODY = `## Criterios

Texto.

## Estado de la implementación (2026-10-07)

Implementado en: PR #1.

## Estado de la implementación (2026-10-08)

Implementado en: PR #10, #12 (ajuste en #13).

Estado: implementación parcial. Pendiente:
- Falta el escenario 3.
- Linux y Windows ([\`xplat\`](../../../../architecture/xplat-pendientes.md)).

Sincronizado por la tarea X.
`;

function writeFixture(root) {
  const f = (rel, text) => {
    const p = join(root, 'docs', 'requirements', 'features', rel);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, text);
  };
  f('alpha/user-stories/US-ALP-002-b.md', card({ id: 'US-ALP-002', title: 'Segunda | con barra', type: 'us', status: 'draft' }));
  f(
    'alpha/user-stories/US-ALP-001-a.md',
    card({ id: 'US-ALP-001', title: 'Primera', type: 'us', status: 'partially-implemented', updated: '2026-10-08', body: PARTIAL_BODY }),
  );
  f(
    'alpha/technical-stories/TS-ALP-001-t.md',
    card({ id: 'TS-ALP-001', title: 'Técnica', type: 'ts', status: 'implemented', extra: 'milestone: m2\n', body: '## Estado de la implementación (2026-10-05)\n\nImplementado en: PR #7.\n' }),
  );
  f('beta/technical-stories/SPIKE-BET-001-s.md', card({ id: 'SPIKE-BET-001', title: 'Spike', type: 'spike', status: 'done' }));
  f('beta/technical-stories/TD-BET-001-d.md', card({ id: 'TD-BET-001', title: 'Deuda', type: 'td', status: 'ready' }));
  f('beta/technical-stories/INF-BET-001-i.md', card({ id: 'INF-BET-001', title: 'Infra', type: 'inf', status: 'blocked' }));
  // Not cards: a dev spec, a context and a research brief.
  f('alpha/dev-specs/US-ALP-001-a.md', card({ id: 'DS-US-ALP-001', title: 'DS', type: 'dev-spec', status: 'approved' }));
  f('alpha/context.md', card({ id: 'CTX-ALP-001', title: 'Ctx', type: 'context', status: 'approved' }));
  f('beta/research/SPIKE-BET-001-res.md', card({ id: 'SPIKE-BET-001-RES', title: 'R', type: 'research', status: 'done' }));
}

describe('parsers', () => {
  test('frontmatter keeps top-level scalars and unquotes strings', () => {
    const fm = parseFrontmatter('---\nid: US-A-001\ntitle: "Hola \\"x\\""\nrelated:\n  stories: [a]\nstatus: draft # comment\n---\nbody');
    assert.deepEqual(fm, { id: 'US-A-001', title: 'Hola "x"', status: 'draft' });
    assert.equal(parseFrontmatter('no frontmatter'), null);
  });

  test('implementation reads the last section only', () => {
    const impl = parseImplementation(PARTIAL_BODY);
    assert.deepEqual(impl.prs, [10, 12, 13]);
    assert.equal(impl.implementedIn, 'PR #10, #12 (ajuste en #13).');
    assert.equal(impl.pending.length, 2);
    assert.equal(impl.pending[0], 'Falta el escenario 3.');
    assert.deepEqual(parseImplementation('# nada'), { hasSection: false, implementedIn: null, prs: [], pending: [] });
  });

  test('release plan assigns ids from table rows under M<n> headings, first milestone wins', () => {
    const plan = parseReleasePlan(
      [
        '# Plan',
        '## Hito M1 — Dogfooding',
        'Fuera de M1: US-ALP-002.',
        '| Item | Estado |',
        '|---|---|',
        '| US-ALP-001, TD-BET-001 | x |',
        '### Detalle',
        '| SPIKE-BET-001 | y |',
        '## M2',
        '| US-ALP-001 | repetida |',
        '| US-ALP-002 | z |',
        '## Fuera del plan',
        '| INF-BET-001 | w |',
      ].join('\n'),
    );
    assert.deepEqual(Object.fromEntries(plan.byId), {
      'US-ALP-001': 'M1',
      'TD-BET-001': 'M1',
      'SPIKE-BET-001': 'M1',
      'US-ALP-002': 'M2',
    });
  });
});

describe('model and report', () => {
  let root;
  before(() => {
    root = mkdtempSync(join(tmpdir(), 'relstatus-'));
    writeFixture(root);
  });
  after(() => rmSync(root, { recursive: true, force: true }));

  test('counts only US, TS, INF, SPIKE and TD cards, by status and by feature', () => {
    const model = buildModel({ root, planPath: join(root, 'missing.md') });
    assert.equal(model.totals.total, 6);
    assert.deepEqual(model.totals.byStatus, {
      implemented: 1,
      done: 1,
      'partially-implemented': 1,
      ready: 1,
      draft: 1,
      blocked: 1,
    });
    assert.deepEqual(
      model.features.map((f) => [f.name, f.total]),
      [['alpha', 3], ['beta', 3]],
    );
    assert.equal(model.lastUpdated, '2026-10-08');
    assert.equal(model.releasePlan, null);
    // The frontmatter milestone works without a plan.
    assert.deepEqual(model.milestones.map((m) => m.name), ['M2']);
  });

  test('without a plan or milestone fields, the report says grouping by milestone is pending', () => {
    const bare = mkdtempSync(join(tmpdir(), 'relstatus-bare-'));
    try {
      mkdirSync(join(bare, 'docs', 'requirements', 'features', 'x', 'user-stories'), { recursive: true });
      writeFileSync(
        join(bare, 'docs', 'requirements', 'features', 'x', 'user-stories', 'US-X-001.md'),
        card({ id: 'US-X-001', title: 'X', type: 'us', status: 'draft' }),
      );
      const md = renderMarkdown(buildModel({ root: bare, planPath: join(bare, 'none.md') }));
      assert.match(md, /Sin agrupado por hito/);
    } finally {
      rmSync(bare, { recursive: true, force: true });
    }
  });

  test('the report groups by milestone from the plan and lists what partial cards lack', () => {
    const planPath = join(root, 'docs', 'requirements', 'release-plan.md');
    writeFileSync(planPath, '# Plan\n\n## M1\n\n| Item |\n|---|\n| US-ALP-001 |\n| TS-ALP-001 |\n');
    try {
      const model = buildModel({ root, planPath });
      // The card's own milestone (M2) wins over the plan (M1).
      assert.equal(model.items.find((i) => i.id === 'TS-ALP-001').milestone, 'M2');
      assert.equal(model.items.find((i) => i.id === 'US-ALP-001').milestone, 'M1');
      const md = renderMarkdown(model);
      assert.match(md, /^<!-- GENERADO: no editar a mano/);
      assert.match(md, /### M1\n\n1 fichas/);
      assert.match(md, /### M2\n/);
      assert.match(md, /### Sin hito\n\n4 fichas/);
      assert.match(md, /Segunda \\\| con barra/);
      assert.match(md, /\| \[US-ALP-001\]\(features\/alpha\/user-stories\/US-ALP-001-a\.md\) \| Primera \| Implementado en parte \(`partially-implemented`\) \| #10, #12, #13 \| 2 \|/);
      assert.match(md, /## Implementadas en parte: lo que falta\n\n### \[US-ALP-001\]/);
      assert.match(md, /- Falta el escenario 3\./);
      // Links in pending bullets are rebased from the card to docs/requirements/.
      assert.match(md, /\(\.\.\/architecture\/xplat-pendientes\.md\)/);
      // Deterministic: same input, same bytes.
      assert.equal(renderMarkdown(buildModel({ root, planPath })), md);
    } finally {
      rmSync(planPath);
    }
  });

  test('CLI: write, check, stale check and JSON', () => {
    const out = join(root, 'docs', 'requirements', 'release-status.md');
    execFileSync(process.execPath, [SCRIPT, '--root', root], { encoding: 'utf8' });
    assert.match(readFileSync(out, 'utf8'), /# Estado de release/);
    assert.equal(spawnSync(process.execPath, [SCRIPT, '--root', root, '--check']).status, 0);

    writeFileSync(out, readFileSync(out, 'utf8') + 'edición a mano\n');
    const stale = spawnSync(process.execPath, [SCRIPT, '--root', root, '--check'], { encoding: 'utf8', env: { ...process.env, GITHUB_ACTIONS: '' } });
    assert.equal(stale.status, 1);
    assert.match(stale.stderr, /out of date\. Regenerate it with `node tools\/status\/release-status\.mjs`/);

    rmSync(out);
    const missing = spawnSync(process.execPath, [SCRIPT, '--root', root, '--check'], { encoding: 'utf8' });
    assert.equal(missing.status, 1);
    assert.match(missing.stderr, /missing/);

    const json = JSON.parse(execFileSync(process.execPath, [SCRIPT, '--root', root, '--json'], { encoding: 'utf8' }));
    assert.equal(json.totals.total, 6);
    assert.equal(json.items.find((i) => i.id === 'US-ALP-001').pending.length, 2);

    assert.equal(spawnSync(process.execPath, [SCRIPT, '--bogus']).status, 2);
  });

  test('an unknown status fails the check; a status section without PRs is a warning', () => {
    const typo = join(root, 'docs', 'requirements', 'features', 'beta', 'user-stories', 'US-BET-009-z.md');
    mkdirSync(dirname(typo), { recursive: true });
    writeFileSync(typo, card({ id: 'US-BET-009', title: 'Z', type: 'us', status: 'implemnted', body: '## Estado de la implementación (2026-10-08)\n\nSin PR.\n' }));
    try {
      const model = buildModel({ root, planPath: join(root, 'none.md') });
      assert.equal(model.errors.length, 1);
      assert.match(model.errors[0], /status desconocido "implemnted"/);
      assert.match(model.warnings.join('\n'), /US-BET-009-z\.md: .*"Implementado en:"/);
      // The write still writes the file but exits 1; the check exits 1 even when the file matches.
      assert.equal(spawnSync(process.execPath, [SCRIPT, '--root', root]).status, 1);
      const check = spawnSync(process.execPath, [SCRIPT, '--root', root, '--check'], { encoding: 'utf8' });
      assert.equal(check.status, 1);
      assert.match(check.stderr, /error: .*status desconocido/);
    } finally {
      rmSync(typo);
    }
  });
});
