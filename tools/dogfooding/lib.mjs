// Shared pieces of the dogfooding log (M1): paths, safety checks and the sample shape.
// No dependencies. Everything here is pure or touches only the dogfooding data directory.
import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SAMPLE_VERSION = 1;

/** Where the samples live: outside every repo and outside the GitRaptor profile (NFR-01). */
export function defaultDataDir(env = process.env) {
  return env.GITRAPTOR_DOGFOODING_DIR || join(homedir(), '.gitraptor-dogfooding');
}

/** Where the reports go: `bitacora/dogfooding/` of the checkout that holds this script. */
export function defaultReportsDir(env = process.env) {
  if (env.GITRAPTOR_DOGFOODING_REPORTS) return env.GITRAPTOR_DOGFOODING_REPORTS;
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
  return join(root, 'bitacora', 'dogfooding');
}

/** The deepest existing ancestor of `path`, resolved through symlinks. */
function existingReal(path) {
  let p = resolve(path);
  const rest = [];
  while (!existsSync(p)) {
    const parent = dirname(p);
    if (parent === p) break;
    rest.unshift(p.slice(parent.length + (parent.endsWith(sep) ? 0 : 1)));
    p = parent;
  }
  return join(realpathSync(p), ...rest);
}

/** The repo (a directory with `.git`) that contains `path`, or null. */
export function enclosingRepo(path) {
  let p = existingReal(path);
  for (;;) {
    if (existsSync(join(p, '.git'))) return p;
    const parent = dirname(p);
    if (parent === p) return null;
    p = parent;
  }
}

// The base folders under which each OS keeps the profile folder `gitraptor` (crates/core,
// profile/dirs.rs): Application Support on macOS, the XDG folders on Linux, %LOCALAPPDATA% on Windows.
const PROFILE_BASES = new Set(['application support', 'caches', 'share', '.config', 'config', 'state', 'local']);

/**
 * Refuses a data directory inside a repo or inside the GitRaptor profile: the samples must never
 * land where GitRaptor or Git keep state (NFR-01). Returns the reason, or null when it is fine.
 */
export function unsafeDataDir(path) {
  const real = existingReal(path);
  const profileRoot = process.env.GITRAPTOR_PROFILE_DIR;
  if (profileRoot && (real + sep).startsWith(existingReal(profileRoot) + sep)) {
    return `${real} is inside the GitRaptor profile`;
  }
  const segments = real.split(sep);
  const inProfile = segments.some(
    (s, i) => s.toLowerCase() === 'gitraptor' && PROFILE_BASES.has(segments[i - 1]?.toLowerCase()),
  );
  if (inProfile) return `${real} is inside the GitRaptor profile`;
  const repo = enclosingRepo(real);
  if (repo) return `${real} is inside the repo ${repo}`;
  return null;
}

/** Local calendar date `YYYY-MM-DD` and time `HH:MM` of a timestamp, in this machine's zone. */
export function localDate(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function localTime(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Stable short id of an event, the one `daily.mjs --mark` takes: `<repo_id 8 chars>:<seq>`. */
export function eventId(repoId, seq) {
  return `${String(repoId).slice(0, 8)}:${seq}`;
}

/**
 * The actor class of an event from `raptor events --json`:
 * `detected` / `registered` (attributed to an agent), `inferred` (unattributed, but the
 * worktree's only active session is suggested) or `none` (no agent).
 */
export function actorClass(event) {
  const a = event.actor;
  if (a && typeof a === 'object' && a.actor === 'agent') {
    return a.origin === 'registered' ? 'registered' : 'detected';
  }
  return event.inferred ? 'inferred' : 'none';
}

/** The agent kind behind an event (`claude-code`, `other`) or null when there is none. */
export function actorKind(event) {
  const a = event.actor;
  if (a && typeof a === 'object' && a.actor === 'agent') return a.kind ?? null;
  if (event.inferred && typeof event.inferred === 'object') return event.inferred.kind ?? null;
  return null;
}

/** Reads a JSON file, or `fallback` when it does not exist. */
export function readJson(path, fallback) {
  if (!existsSync(path)) return fallback;
  return JSON.parse(readFileSync(path, 'utf8'));
}

/** Reads a `.jsonl` file; a torn last line (a sample cut by sleep or a kill) is skipped. */
export function readJsonl(path) {
  if (!existsSync(path)) return [];
  const out = [];
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    try {
      out.push(JSON.parse(line));
    } catch {
      // A partial line: ignore it, the next sample is complete.
    }
  }
  return out;
}
