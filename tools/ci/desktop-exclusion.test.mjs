// CI1 (TS-DSK-001 T008): the general jobs exclude gitraptor-desktop and the `desktop` workflow exists.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (rel) => readFileSync(path.join(root, rel), 'utf8');
const EXCLUDE = '--exclude gitraptor-desktop';

test('CI1: cargo-test-selection.sh without SCOPE prints args=--workspace --exclude gitraptor-desktop', () => {
  const env = { ...process.env, SCOPE: '', PROJECTS: '', GITHUB_OUTPUT: '' };
  const r = spawnSync('bash', ['tools/ci/cargo-test-selection.sh'], { cwd: root, encoding: 'utf8', env });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /^args=--workspace --exclude gitraptor-desktop$/m, 'whole-workspace output must exclude gitraptor-desktop');
});

test('CI1: cargo-test-selection.sh never emits a bare --workspace and skips the crate in its member loop', () => {
  const src = read('tools/ci/cargo-test-selection.sh');
  for (const line of src.split('\n')) {
    if (/^\s*emit --workspace\s*$/.test(line)) assert.fail(`bare "emit --workspace" without exclude: ${line.trim()}`);
  }
  assert.match(
    src,
    /selection=\(--workspace --exclude gitraptor-desktop\)|selection=\(--workspace\)[\s\S]*selection\+=\(--exclude gitraptor-desktop\)/,
    'the selection must start with the exclusion of gitraptor-desktop',
  );
  assert.match(src, /tree --workspace --exclude gitraptor-desktop/, 'tree --workspace must carry the exclusion');
});

test('CI1: nextest-junit default arguments exclude gitraptor-desktop', () => {
  const src = read('tools/test/nextest-junit.mjs');
  assert.match(src, /\["--workspace",\s*"--exclude",\s*"gitraptor-desktop"\]/, 'default args must be ["--workspace", "--exclude", "gitraptor-desktop"]');
});

test('CI1: affected-tests.mjs never emits gitraptor-desktop', () => {
  const src = read('tools/ci/affected-tests.mjs');
  assert.match(src, /['"]gitraptor-desktop['"]/, 'affected-tests.mjs must filter gitraptor-desktop out of the affected list');
});

test('CI1: repo-intact.yml filters apps/cli and apps/mcp, not apps/**, and excludes the crate everywhere', () => {
  const yml = read('.github/workflows/repo-intact.yml');
  assert.doesNotMatch(yml, /- 'apps\/\*\*'/, 'the rust filter must not use apps/**');
  assert.match(yml, /- 'apps\/cli\/\*\*'/, 'rust filter lacks apps/cli/**');
  assert.match(yml, /- 'apps\/mcp\/\*\*'/, 'rust filter lacks apps/mcp/**');
  const lines = yml.split('\n').filter((l) => /cargo (clippy|test)\b.*--workspace/.test(l));
  assert.ok(lines.length >= 4, `expected the clippy and test --workspace lines, found ${lines.length}`);
  for (const l of lines) assert.ok(l.includes(EXCLUDE), `missing ${EXCLUDE}: ${l.trim()}`);
  assert.match(yml, /'--workspace --exclude gitraptor-desktop'/, 'the fallback of the test step must exclude the crate');
});

test('CI1: .github/workflows/desktop.yml exists with name, path filter and ordered steps', () => {
  const file = '.github/workflows/desktop.yml';
  assert.ok(existsSync(path.join(root, file)), `${file} does not exist`);
  const yml = read(file);
  assert.match(yml, /^name: desktop$/m);
  const filters = [
    'apps/desktop/**',
    'packages/design-tokens/**',
    'crates/api/**',
    'Cargo.toml',
    'Cargo.lock',
    'rust-toolchain.toml',
    'package.json',
    'pnpm-lock.yaml',
    'pnpm-workspace.yaml',
    '.github/workflows/desktop.yml',
  ];
  for (const p of filters) assert.ok(yml.includes(`- '${p}'`), `path filter lacks ${p}`);
  assert.match(yml, /timeout-minutes: 45/);
  const steps = [
    'libwebkit2gtk-4.1-dev',
    'rustup show',
    'pnpm install --frozen-lockfile --ignore-scripts',
    'pnpm audit',
    'pnpm --filter @gitraptor/desktop lint',
    'pnpm --filter @gitraptor/desktop typecheck',
    'pnpm --filter @gitraptor/desktop test',
    'pnpm --filter @gitraptor/desktop build',
    'cargo clippy -p gitraptor-desktop --all-targets -- -D warnings',
    'cargo test -p gitraptor-desktop',
  ];
  let at = -1;
  for (const s of steps) {
    const i = yml.indexOf(s, at + 1);
    assert.ok(i > at, `step "${s}" missing or out of order`);
    at = i;
  }
});
