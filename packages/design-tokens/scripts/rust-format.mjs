// Style Dictionary format that writes `crates/theme/src/generated.rs` (ADR-GRP-003, ADR-CKP-003 § 10).
//
// The output is plain data: the semantic color tokens with their three depths (truecolor, 256,
// 16) for the normal and the high-contrast sets, and the symbols with glyph, ASCII fallback and
// width. `crates/theme` owns the types and the resolution logic.

export const EXT = 'dev.gitraptor';

const ANSI16 = [
  'black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white',
  'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta',
  'brightCyan', 'brightWhite',
];
const ATTRS = { bold: 'BOLD', dim: 'DIM', reverse: 'REVERSE', underline: 'UNDERLINE' };
const ROLES = { foreground: 'Foreground', background: 'Background' };

const CUBE = [0, 95, 135, 175, 215, 255];

/** Parses `#rrggbb` into `[r, g, b]`. */
export function hexToRgb(hex) {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) throw new Error(`not a #rrggbb color: ${hex}`);
  const n = Number.parseInt(m[1], 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

/** The RGB value of an xterm 256-color index in 16..=255. */
export function ansi256Rgb(index) {
  if (!Number.isInteger(index) || index < 16 || index > 255) throw new Error(`not an index in 16..=255: ${index}`);
  if (index >= 232) {
    const v = 8 + 10 * (index - 232);
    return [v, v, v];
  }
  const i = index - 16;
  return [CUBE[Math.floor(i / 36)], CUBE[Math.floor(i / 6) % 6], CUBE[i % 6]];
}

/** sRGB (0..255) to OKLab, for a perceptual nearest-color search. */
function oklab([r, g, b]) {
  const lin = (c) => {
    const v = c / 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  const [lr, lg, lb] = [lin(r), lin(g), lin(b)];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

/**
 * Nearest xterm 256-color index, by OKLab distance. Only 16..=255 (color cube and gray ramp):
 * the first 16 are remapped by every terminal theme, so they are never a faithful approximation.
 */
export function nearestAnsi256(rgb) {
  const [L, A, B] = oklab(rgb);
  let best = 16;
  let bestDist = Infinity;
  for (let index = 16; index <= 255; index++) {
    const [l, a, b] = oklab(ansi256Rgb(index));
    const d = (L - l) ** 2 + (A - a) ** 2 + (B - b) ** 2;
    if (d < bestDist) {
      bestDist = d;
      best = index;
    }
  }
  return best;
}

const pascal = (path) =>
  path.map((p) => p.charAt(0).toUpperCase() + p.slice(1)).join('');

const isSemantic = (token) => /[\\/]semantic\.json$/.test(token.filePath);
const isSymbol = (token) => /[\\/]symbol\.json$/.test(token.filePath);

const refPath = (ref, owner) => {
  const m = /^\{([^}]+)\}$/.exec(ref ?? '');
  if (!m) throw new Error(`${owner}: expected a {reference}, got ${ref}`);
  return m[1];
};

/** Resolves a `{color.x.y}` reference to a primitive: its hex and its three depths. */
function primitive(ref, byPath, owner) {
  const p = refPath(ref, owner);
  const target = byPath.get(p);
  if (!target || isSemantic(target)) throw new Error(`${owner}: ${ref} is not a primitive`);
  const ext = target.$extensions?.[EXT] ?? {};
  if (!ANSI16.includes(ext.ansi16)) throw new Error(`${p}: missing or invalid ansi16 '${ext.ansi16}'`);
  const rgb = hexToRgb(target.$value);
  const ansi256 = ext.ansi256 ?? nearestAnsi256(rgb);
  return { rgb, ansi256, ansi256Rgb: ansi256Rgb(ansi256), ansi16: ext.ansi16 };
}

const values = ({ rgb, ansi256, ansi256Rgb: rgb256, ansi16 }) =>
  `Values { rgb: Rgb(${rgb.join(', ')}), ansi256: ${ansi256}, ansi256_rgb: Rgb(${rgb256.join(', ')}), ansi16: Ansi16::${pascal([ansi16])} }`;

/**
 * The semantic token a token stands for: itself, or the semantic it aliases (`$value` pointing to
 * another semantic, without extensions of its own).
 */
function definition(token, byPath) {
  const seen = new Set();
  let t = token;
  for (;;) {
    const target = byPath.get(refPath(t.original.$value, t.path.join('.')));
    if (!target || !isSemantic(target)) return t;
    if (t.original.$extensions?.[EXT]) throw new Error(`${t.path.join('.')}: an alias cannot have extensions`);
    if (seen.has(target)) throw new Error(`${token.path.join('.')}: circular alias`);
    seen.add(target);
    t = target;
  }
}

function attrs(list, owner) {
  if (!list || list.length === 0) return 'Attrs::NONE';
  return list
    .map((a) => {
      if (!ATTRS[a]) throw new Error(`${owner}: unknown noColor attribute '${a}'`);
      return `Attrs::${ATTRS[a]}`;
    })
    .reduce((acc, a) => `${acc}.union(${a})`);
}

const rustStr = (s) => JSON.stringify(s);
const doc = (s) => (s ? s.replace(/\s+/g, ' ').trim() : '');

/** Builds the Rust source from the dictionary. Pure: same tokens, same bytes. */
export function renderRust(dictionary) {
  const byPath = new Map(dictionary.allTokens.map((t) => [t.path.join('.'), t]));
  const colors = dictionary.allTokens.filter(isSemantic);
  const symbols = dictionary.allTokens.filter(isSymbol);

  const colorRows = colors.map((t) => {
    const name = t.path.slice(1).join('.');
    const def = definition(t, byPath);
    const ext = def.original.$extensions?.[EXT] ?? {};
    const role = ROLES[ext.role ?? 'foreground'];
    if (!role) throw new Error(`${name}: unknown role '${ext.role}'`);
    return {
      variant: pascal(t.path.slice(1)),
      name,
      description: doc(t.$description),
      spec: [
        `ColorSpec {`,
        `        role: Role::${role},`,
        `        inherit: ${ext.inherit === true},`,
        `        no_color: ${attrs(ext.noColor, name)},`,
        `        normal: ${values(primitive(def.original.$value, byPath, name))},`,
        `        high_contrast: ${values(primitive(ext.highContrast, byPath, `${name} (highContrast)`))},`,
        `    }`,
      ].join('\n'),
    };
  });

  const symbolRows = symbols.map((t) => {
    const name = t.path.slice(1).join('.');
    const { glyph, ascii, width } = t.$value;
    if (typeof glyph !== 'string' || typeof ascii !== 'string' || !Number.isInteger(width)) {
      throw new Error(`${name}: a symbol needs glyph, ascii and width`);
    }
    return {
      variant: pascal(t.path.slice(1)),
      name,
      description: doc(t.$description),
      spec: `SymbolSpec { glyph: ${rustStr(glyph)}, glyph_width: ${width}, ascii: ${rustStr(ascii)} }`,
    };
  });

  const out = [];
  out.push(
    '// @generated by packages/design-tokens (Style Dictionary). Do not edit.',
    '// Regenerate with `pnpm nx build @gitraptor/tokens`; CI fails if this file is stale.',
    '',
    'use crate::{Ansi16, Attrs, ColorSpec, Rgb, Role, SymbolSpec, Values};',
    '',
  );
  emitEnum(out, 'ColorToken', 'A semantic color token (DSYS-GRP-001 § 2.1).', colorRows, 'ColorSpec', 'COLOR_SPECS');
  emitEnum(out, 'SymbolToken', 'A symbol token (DSYS-GRP-001 § 2.2).', symbolRows, 'SymbolSpec', 'SYMBOL_SPECS');
  return out.join('\n');
}

function emitEnum(out, ty, docLine, rows, specTy, specConst) {
  out.push(
    `/// ${docLine}`,
    '#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]',
    `pub enum ${ty} {`,
  );
  for (const r of rows) {
    out.push(`    /// \`${r.name}\`${r.description ? `: ${r.description}` : ''}`, `    ${r.variant},`);
  }
  out.push('}', '', `impl ${ty} {`, '    /// Every token, in declaration order.');
  out.push(`    pub const ALL: [${ty}; ${rows.length}] = [`);
  for (const r of rows) out.push(`        ${ty}::${r.variant},`);
  out.push('    ];', '', '    /// The token name in `packages/design-tokens`.');
  out.push('    pub const fn name(self) -> &\'static str {', '        match self {');
  for (const r of rows) out.push(`            ${ty}::${r.variant} => ${rustStr(r.name)},`);
  out.push('        }', '    }', '', `    pub(crate) const fn spec(self) -> &'static ${specTy} {`);
  out.push(`        &${specConst}[self as usize]`, '    }', '}', '');
  out.push(`const ${specConst}: [${specTy}; ${rows.length}] = [`);
  for (const r of rows) out.push(`    // ${r.name}`, `    ${r.spec},`);
  out.push('];', '');
}
