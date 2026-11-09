// Builds the tokens: `build/tokens.json`, the Rust module of `crates/theme` and `css/glass.css`
// (ADR-GRP-003 and its Enmienda 2026-11-08).
//
//   node scripts/build.mjs            write every output in place
//   node scripts/build.mjs --check    regenerate into a temp dir and fail if `crates/theme` or `css/glass.css` is stale
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import StyleDictionary from 'style-dictionary';
import { renderCss } from './css-format.mjs';
import { renderRust } from './rust-format.mjs';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const DEFAULT_TOKENS_DIR = path.join(pkgRoot, 'tokens');
export const DEFAULT_RUST_FILE = path.resolve(pkgRoot, '../../crates/theme/src/generated.rs');
export const DEFAULT_CSS_FILE = path.join(pkgRoot, 'css', 'glass.css');
const RUST_FORMAT = 'gitraptor/rust-theme';
const CSS_FORMAT = 'gitraptor/css-glass';
const isGlass = (token) => token.path[0] === 'glass';

/**
 * Runs Style Dictionary over `tokensDir`, writing `rustFile` (the non-`glass` tokens), `cssFile` (the
 * `glass` tokens) if given, and `jsonDir/tokens.json` if given.
 */
export async function buildTokens({ tokensDir = DEFAULT_TOKENS_DIR, rustFile = DEFAULT_RUST_FILE, cssFile, jsonDir } = {}) {
  const platforms = {
    rust: {
      transforms: ['name/kebab'],
      buildPath: `${path.dirname(rustFile)}${path.sep}`,
      files: [{ destination: path.basename(rustFile), format: RUST_FORMAT, filter: (token) => !isGlass(token) }],
    },
  };
  if (cssFile) {
    platforms.css = {
      transforms: ['name/kebab'],
      buildPath: `${path.dirname(cssFile)}${path.sep}`,
      files: [{ destination: path.basename(cssFile), format: CSS_FORMAT, filter: isGlass }],
    };
  }
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
  sd.registerFormat({ name: CSS_FORMAT, format: ({ dictionary }) => renderCss(dictionary) });
  await sd.buildAllPlatforms();
}

/**
 * Regenerates into a temp dir and lists the committed outputs that differ from it (`committedFile`
 * for Rust, `committedCss` for the glass CSS). An empty list means up to date.
 */
export async function staleOutputs({
  tokensDir = DEFAULT_TOKENS_DIR,
  committedFile = DEFAULT_RUST_FILE,
  committedCss = DEFAULT_CSS_FILE,
} = {}) {
  const dir = await mkdtemp(path.join(tmpdir(), 'gitraptor-tokens-'));
  try {
    const rustFile = path.join(dir, 'generated.rs');
    const cssFile = path.join(dir, 'glass.css');
    await buildTokens({ tokensDir, rustFile, cssFile });
    const stale = [];
    for (const [fresh, committed] of [
      [rustFile, committedFile],
      [cssFile, committedCss],
    ]) {
      const [a, b] = await Promise.all([readFile(fresh), readFile(committed).catch(() => null)]);
      if (b === null || !a.equals(b)) stale.push(committed);
    }
    return stale;
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Resolves to `true` when every committed output is up to date with the tokens. */
export async function isUpToDate(options = {}) {
  return (await staleOutputs(options)).length === 0;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.includes('--check')) {
    const stale = await staleOutputs();
    if (stale.length > 0) {
      for (const file of stale) {
        console.error(`${path.relative(process.cwd(), file)} is stale: run \`pnpm nx build @gitraptor/tokens\` and commit it.`);
      }
      process.exit(1);
    }
    console.log('crates/theme and css/glass.css are up to date with the tokens.');
  } else {
    await buildTokens({ cssFile: DEFAULT_CSS_FILE, jsonDir: path.join(pkgRoot, 'build') });
  }
}
