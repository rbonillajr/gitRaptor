// TS-CKP-004: completeness of the tokens and the "generated code is up to date" control.
import assert from 'node:assert/strict';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { DEFAULT_TOKENS_DIR, buildTokens, isUpToDate } from '../scripts/build.mjs';
import { EXT, ansi256Rgb, hexToRgb, nearestAnsi256 } from '../scripts/rust-format.mjs';

const readJson = async (name) => JSON.parse(await readFile(path.join(DEFAULT_TOKENS_DIR, name), 'utf8'));

/** Leaf tokens (objects with `$value`) as `[dottedPath, token]`. */
function leaves(node, prefix = []) {
  if (node && typeof node === 'object' && '$value' in node) return [[prefix.join('.'), node]];
  return Object.entries(node ?? {})
    .filter(([k]) => !k.startsWith('$'))
    .flatMap(([k, v]) => leaves(v, [...prefix, k]));
}

async function withTempTokens(fn) {
  const dir = await mkdtemp(path.join(tmpdir(), 'gitraptor-tokens-test-'));
  try {
    await cp(DEFAULT_TOKENS_DIR, path.join(dir, 'tokens'), { recursive: true });
    return await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

test('the committed crates/theme module is up to date with the tokens', async () => {
  assert.equal(await isUpToDate(), true, 'run `pnpm nx build @gitraptor/tokens` and commit');
});

test('the control fails when a token is edited without regenerating', async () => {
  await withTempTokens(async (dir) => {
    const file = path.join(dir, 'tokens', 'color.json');
    const json = JSON.parse(await readFile(file, 'utf8'));
    json.color.green['500'].$value = '#00ff00';
    await writeFile(file, JSON.stringify(json));
    assert.equal(await isUpToDate({ tokensDir: path.join(dir, 'tokens') }), false);
  });
});

test('the control fails when a symbol is edited without regenerating', async () => {
  await withTempTokens(async (dir) => {
    const file = path.join(dir, 'tokens', 'symbol.json');
    const json = JSON.parse(await readFile(file, 'utf8'));
    json.symbol.conflict.$value.ascii = '[C]';
    await writeFile(file, JSON.stringify(json));
    assert.equal(await isUpToDate({ tokensDir: path.join(dir, 'tokens') }), false);
  });
});

test('generation is deterministic', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'gitraptor-tokens-det-'));
  try {
    const [a, b] = [path.join(dir, 'a.rs'), path.join(dir, 'b.rs')];
    await buildTokens({ rustFile: a });
    await buildTokens({ rustFile: b });
    const [ba, bb] = await Promise.all([readFile(a), readFile(b)]);
    assert.ok(ba.equals(bb));
    assert.ok(!ba.includes('\r'), 'LF line endings only');
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('every semantic token references primitives for both sets, or aliases a semantic', async () => {
  const primitives = new Map(leaves((await readJson('color.json'))));
  const semantic = new Map(leaves((await readJson('semantic.json'))));
  const ref = (v) => /^\{([^}]+)\}$/.exec(v)?.[1];
  assert.ok(semantic.size >= 28);
  for (const [name, token] of semantic) {
    const target = ref(token.$value);
    assert.ok(target, `${name}: $value must be a reference`);
    if (semantic.has(target)) {
      assert.equal(token.$extensions, undefined, `${name}: an alias has no extensions`);
      continue;
    }
    const ext = token.$extensions?.[EXT];
    assert.ok(primitives.has(target), `${name}: ${target} is not a primitive`);
    assert.ok(primitives.has(ref(ext?.highContrast)), `${name}: missing high-contrast value`);
  }
  for (const [name, token] of primitives) {
    assert.match(token.$value, /^#[0-9a-f]{6}$/, name);
    assert.ok(token.$extensions?.[EXT]?.ansi16, `${name}: missing 16-color fallback`);
  }
});

test('every symbol has glyph, pure-ASCII fallback and width', async () => {
  const symbols = leaves((await readJson('symbol.json')));
  assert.ok(symbols.length >= 10);
  for (const [name, { $value: s }] of symbols) {
    assert.ok(s.glyph && !/^[\x20-\x7e]*$/.test(s.glyph), `${name}: glyph`);
    assert.match(s.ascii, /^[\x21-\x7e]+$/, `${name}: fallback must be printable ASCII`);
    assert.ok([1, 2].includes(s.width), `${name}: width`);
  }
});

test('the derived 256-color index never uses the user-remapped 0..15', () => {
  for (const hex of ['#000000', '#ffffff', '#ff0000', '#00ff00', '#0000ff', '#1c1c1c', '#808080']) {
    const index = nearestAnsi256(hexToRgb(hex));
    assert.ok(index >= 16 && index <= 255, `${hex} -> ${index}`);
  }
  assert.equal(nearestAnsi256([0, 0, 0]), 16);
  assert.equal(nearestAnsi256([255, 255, 255]), 231);
  assert.deepEqual(ansi256Rgb(244), [128, 128, 128]);
});
