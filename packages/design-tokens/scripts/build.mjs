// Builds the tokens: `build/tokens.json` and the Rust module of `crates/theme` (ADR-GRP-003).
//
//   node scripts/build.mjs            write both outputs in place
//   node scripts/build.mjs --check    regenerate into a temp dir and fail if `crates/theme` is stale
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import StyleDictionary from 'style-dictionary';
import { renderRust } from './rust-format.mjs';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const DEFAULT_TOKENS_DIR = path.join(pkgRoot, 'tokens');
export const DEFAULT_RUST_FILE = path.resolve(pkgRoot, '../../crates/theme/src/generated.rs');
const RUST_FORMAT = 'gitraptor/rust-theme';

/** Runs Style Dictionary over `tokensDir`, writing `rustFile` (and `jsonDir/tokens.json` if given). */
export async function buildTokens({ tokensDir = DEFAULT_TOKENS_DIR, rustFile = DEFAULT_RUST_FILE, jsonDir } = {}) {
  const platforms = {
    rust: {
      transforms: ['name/kebab'],
      buildPath: `${path.dirname(rustFile)}${path.sep}`,
      files: [{ destination: path.basename(rustFile), format: RUST_FORMAT }],
    },
  };
  if (jsonDir) {
    platforms.json = {
      transformGroup: 'js',
      buildPath: `${jsonDir}${path.sep}`,
      files: [{ destination: 'tokens.json', format: 'json/nested' }],
    };
  }
  const sd = new StyleDictionary({
    source: [path.join(tokensDir, '**/*.json').split(path.sep).join('/')],
    log: { warnings: 'error', verbosity: 'silent' },
    platforms,
  });
  sd.registerFormat({ name: RUST_FORMAT, format: ({ dictionary }) => renderRust(dictionary) });
  await sd.buildAllPlatforms();
}

/** Regenerates into a temp dir and compares with `committedFile`. Resolves to `true` when up to date. */
export async function isUpToDate({ tokensDir = DEFAULT_TOKENS_DIR, committedFile = DEFAULT_RUST_FILE } = {}) {
  const dir = await mkdtemp(path.join(tmpdir(), 'gitraptor-tokens-'));
  try {
    const fresh = path.join(dir, 'generated.rs');
    await buildTokens({ tokensDir, rustFile: fresh });
    const [a, b] = await Promise.all([readFile(fresh), readFile(committedFile).catch(() => null)]);
    return b !== null && a.equals(b);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.includes('--check')) {
    if (!(await isUpToDate())) {
      console.error(
        `${path.relative(process.cwd(), DEFAULT_RUST_FILE)} is stale: run \`pnpm nx build @gitraptor/tokens\` and commit it.`,
      );
      process.exit(1);
    }
    console.log('crates/theme is up to date with the tokens.');
  } else {
    await buildTokens({ jsonDir: path.join(pkgRoot, 'build') });
  }
}
