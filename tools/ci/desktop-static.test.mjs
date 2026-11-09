// STA1 (TS-DSK-001 T005, T008): the static layer of the desktop job: install, lint, typecheck, build
// and clippy of gitraptor-desktop all exit 0. Sequential: each step needs the one before it.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const TIMEOUT = 1_200_000;

function step(name, cmd, args) {
  test(`STA1: ${name}`, { timeout: TIMEOUT }, () => {
    const env = { ...process.env, CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || '4' };
    const r = spawnSync(cmd, args, { cwd: root, env, encoding: 'utf8', timeout: TIMEOUT, maxBuffer: 64 * 1024 * 1024 });
    const tail = `${r.stdout ?? ''}${r.stderr ?? ''}`.trim().split('\n').slice(-20).join('\n');
    assert.equal(r.status, 0, `${cmd} ${args.join(' ')} exited ${r.status}\n${tail}`);
    // pnpm exits 0 when --filter matches nothing: that is not a pass.
    assert.doesNotMatch(tail, /No projects matched/, `${cmd} ${args.join(' ')} ran no project`);
  });
}

step('pnpm install --frozen-lockfile --ignore-scripts', 'pnpm', ['install', '--frozen-lockfile', '--ignore-scripts']);
step('lint of @gitraptor/desktop', 'pnpm', ['--filter', '@gitraptor/desktop', 'run', 'lint']);
step('typecheck of @gitraptor/desktop', 'pnpm', ['--filter', '@gitraptor/desktop', 'run', 'typecheck']);
step('build of @gitraptor/desktop', 'pnpm', ['--filter', '@gitraptor/desktop', 'run', 'build']);
step('cargo clippy of gitraptor-desktop', 'cargo', ['clippy', '-p', 'gitraptor-desktop', '--all-targets', '--', '-D', 'warnings']);
