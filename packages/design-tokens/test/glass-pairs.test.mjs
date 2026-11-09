// TS003-TOKENS (TS-DSK-003 T001): contrast pairs of primitives-spec § 1.4 (Dev Spec § Pares de contraste),
// in both themes, with translucent colours composited over their surface.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const css = await readFile(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'css', 'glass.css'), 'utf8');

function block(head) {
  const start = css.indexOf(head);
  assert.notEqual(start, -1, `glass.css has no block starting with ${head}`);
  const open = css.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < css.length; i += 1) {
    if (css[i] === '{') depth += 1;
    if (css[i] === '}') depth -= 1;
    if (depth === 0) return css.slice(open + 1, i);
  }
  throw new Error(`unbalanced block ${head}`);
}
const vars = (body) => new Map([...body.matchAll(/(--gr-[a-z0-9-]+)\s*:\s*([^;]*);/g)].map((m) => [m[1], m[2].trim()]));

const dark = vars(block(':root {'));
const light = new Map([...dark, ...vars(block(':root[data-theme="light"]'))]);

function parse(value) {
  if (!value) return null;
  const hex = /^#([0-9a-f]{6})$/i.exec(value);
  if (hex) return { rgb: [0, 2, 4].map((i) => parseInt(hex[1].slice(i, i + 2), 16)), a: 1 };
  const m = /^rgba?\(([^)]+)\)$/.exec(value);
  if (!m) return null;
  const p = m[1].split(',').map((s) => parseFloat(s));
  return { rgb: p.slice(0, 3), a: p[3] ?? 1 };
}
const over = (fg, bg) => ({ rgb: fg.rgb.map((c, i) => fg.a * c + (1 - fg.a) * bg.rgb[i]), a: 1 });
const lum = ({ rgb }) => {
  const [r, g, b] = rgb.map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a, b) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

function check(theme, tokens) {
  const fails = [];
  const colour = (n) => {
    const c = parse(tokens.get(`--gr-${n}`));
    if (!c) fails.push(`${theme}: token --gr-${n} is missing or not a colour`);
    return c;
  };
  const pair = (id, fgName, bgName, base, min) => {
    const fg = colour(fgName);
    let bg = colour(bgName);
    const under = base ? colour(`surface-${base}`) : null;
    if (!fg || !bg || (base && !under)) return;
    if (under) bg = over(bg, under);
    const r = ratio(fg.a < 1 ? over(fg, bg) : fg, bg);
    if (r < min) fails.push(`${id} ${theme}: ${fgName} on ${bgName}${base ? `/${base}` : ''} is ${r.toFixed(2)}:1 (< ${min})`);
  };
  const S = (n) => `surface-${n}`;
  for (const bg of ['accent', 'accent-hover']) pair('C1', 'text-on-accent', bg, null, 4.5);
  for (const s of ['window', 'panel', 'control', 'raised', 'hover']) pair('C2', 'accent-text', S(s), null, 4.5);
  for (const b of ['panel', 'control']) {
    for (const t of ['success', 'warning', 'danger', 'live']) pair('C5', 'text-main', `${t}-bg`, b, 4.5);
    for (const t of ['success', 'warning', 'danger']) pair('C5g', `${t}-text`, `${t}-bg`, b, 3);
    pair('C6', 'text-main', 'accent-glow', b, 4.5);
    pair('C12l', 'live', 'live-bg', b, 3);
  }
  pair('C7', 'text-muted', 'accent-glow', 'panel', 4.5);
  pair('C8', 'danger-text', S('control'), null, 4.5);
  pair('C8', 'danger-text', 'danger-bg', 'control', 4.5);
  for (const s of ['panel', 'control', 'raised']) pair('C9', 'text-faint', S(s), null, 3);
  for (const s of ['window', 'panel', 'control', 'raised', 'hover']) pair('C10', 'accent', S(s), null, 3);
  for (const n of [1, 2, 3, 4, 5, 7]) pair('C11', 'text-on-accent', `lane-${n}`, null, 4.5);
  for (const t of ['success', 'danger', 'text-muted', 'live']) for (const s of ['panel', 'control']) pair('C12', t, S(s), null, 3);
  for (const t of ['text-main', 'text-muted']) pair('C14', t, S('raised'), null, 4.5);
  return fails;
}

for (const [theme, tokens] of [['dark', dark], ['light', light]]) {
  test(`TS003-TOKENS: every contrast pair of the spec meets its threshold in the ${theme} theme`, () => {
    const fails = check(theme, tokens);
    assert.deepEqual(fails, [], `TS003-TOKENS contrast failures:\n${fails.join('\n')}`);
  });
}
