#!/usr/bin/env node
// Which Cargo packages a pull request affects, from the Nx project graph (ADR-GRP-002, Enmienda
// 2026-10-07). Nx decides what runs; cargo runs it in one invocation (`repo-intact.yml`).
//
//   BASE=<sha> HEAD=<sha> node tools/ci/affected-tests.mjs
//
// Writes `scope` (`full` or `affected`), `projects` (space-separated Cargo package names) and
// `graph_ok` (`true` or `false`) to $GITHUB_OUTPUT, or prints them without it. Every doubt
// resolves to `full`: a change to a file the whole workspace reads, an empty affected list, or a
// graph that misses a package a test launches. Read-only.
import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';

// Files every package reads, or that decide how the tests run. Nx can leave some of them out of
// every project (a workflow file), so they are matched here and force the whole workspace.
const GLOBAL = [
  /^Cargo\.(toml|lock)$/,
  /^rust-toolchain(\.toml)?$/,
  /^\.cargo\//,
  /^\.?rustfmt\.toml$/,
  /^\.?clippy\.toml$/,
  /^\.gitattributes$/,
  /^nx\.json$/,
  /^package\.json$/,
  /^pnpm-(lock|workspace)\.yaml$/,
  /^\.github\/workflows\/repo-intact\.yml$/,
  /^tools\/ci\//,
];

const run = (cmd, args) =>
  execFileSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
const nx = (...args) => run('pnpm', ['exec', 'nx', ...args]);

function emit(outputs) {
  const lines = Object.entries(outputs).map(([k, v]) => `${k}=${v}`);
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, lines.join('\n') + '\n');
  console.log(lines.join('\n'));
}

function rustFiles(dir) {
  let out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out = out.concat(rustFiles(path));
    else if (name.endsWith('.rs')) out.push(path);
  }
  return out;
}

// A test that launches another package's binary (`gitraptor_testkit::sibling_bin`) must depend
// on that package in the graph, or a change to that package would not run the test.
function missingEdges(members, graph) {
  const deps = (from, seen = new Set()) => {
    for (const d of graph.dependencies[from] ?? []) {
      if (!seen.has(d.target)) {
        seen.add(d.target);
        deps(d.target, seen);
      }
    }
    return seen;
  };
  const missing = [];
  for (const [name, dir] of members) {
    const reach = deps(name);
    for (const sub of ['tests', 'src', 'benches']) {
      let files = [];
      try {
        files = rustFiles(join(dir, sub));
      } catch {
        continue;
      }
      for (const file of files) {
        for (const m of readFileSync(file, 'utf8').matchAll(/sibling_bin\([^,]+,\s*"([a-z0-9-]+)"/g)) {
          if (m[1] !== name && !reach.has(m[1])) missing.push(`${relative(process.cwd(), file)}: ${name} launches ${m[1]}`);
        }
      }
    }
  }
  return missing;
}

const { BASE: base, HEAD: head } = process.env;
if (!base || !head) {
  console.error('BASE and HEAD are required');
  process.exit(2);
}

const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1']));
const members = new Map(
  metadata.packages
    .filter((p) => metadata.workspace_members.includes(p.id))
    .map((p) => [p.name, p.manifest_path.replace(/[\\/]Cargo\.toml$/, '')]),
);

const tmp = mkdtempSync(join(tmpdir(), 'nx-graph-'));
let graph;
try {
  nx('graph', `--file=${join(tmp, 'graph.json')}`);
  graph = JSON.parse(readFileSync(join(tmp, 'graph.json'), 'utf8')).graph;
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
const missing = missingEdges(members, graph);
for (const m of missing) {
  console.log(`::error::the Nx graph misses a dependency a test needs (add it to implicitDependencies): ${m}`);
}
const graphOk = missing.length === 0;

const changed = run('git', ['diff', '--name-only', '--no-renames', base, head]).split('\n').filter(Boolean);
const global = changed.filter((f) => GLOBAL.some((re) => re.test(f)));
// Plus the packages whose binaries the affected tests launch (`implicitDependencies`): their own
// tests cost little, and with them the selection resolves the same features as the workspace build.
const marked = JSON.parse(nx('show', 'projects', '--affected', `--base=${base}`, `--head=${head}`, '-t', 'test', '--json'));
const launched = marked.flatMap((p) => (graph.dependencies[p] ?? []).filter((d) => d.type === 'implicit').map((d) => d.target));
const affected = [...new Set([...marked, ...launched])].filter((p) => members.has(p)).sort();

let scope = 'affected';
if (!graphOk) scope = 'full';
else if (global.length > 0) {
  console.log(`Whole workspace: ${global.join(', ')} changed`);
  scope = 'full';
} else if (affected.length === 0) {
  console.log('Whole workspace: Nx marked no Cargo package as affected');
  scope = 'full';
}
console.log(`Affected Cargo packages: ${affected.join(' ') || 'none'}`);
emit({ scope, projects: affected.join(' '), graph_ok: String(graphOk) });
