#!/usr/bin/env node
// Adaptador de JUnit de cargo-nextest para `/nassa-core:implement`.
//
// nextest nombra cada <testsuite> por el binario (`gitraptor-core::us_tmc_013`, `gitraptor-core`),
// pero el plugin reconoce un test "del contrato" por el nombre del <testsuite>: con `/` lo trata como
// ruta de archivo y lo compara con `criteria[].tests`. Este script corre nextest y reescribe el
// informe con un <testsuite> por archivo fuente (nombre = ruta relativa a la raíz del repo).
//
// Uso:  node tools/test/nextest-junit.mjs [argumentos de nextest]   (por defecto: --workspace)
//   lee    target/nextest/ci/junit.xml
//   escribe target/nextest/ci/junit-paths.xml
//   sale con el mismo código que `cargo nextest run --profile ci`.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const posix = (p) => p.split(sep).join("/");

/**
 * Índice de binarios de nextest → archivo raíz, desde `cargo metadata --no-deps`.
 * Los ids de binario de nextest: `<paquete>` (lib, proc-macro), `<paquete>::<test>` (integración),
 * `<paquete>::bin/<nombre>`, `<paquete>::bench/<nombre>`, `<paquete>::example/<nombre>`.
 */
export function buildIndex(metadata) {
  const byId = new Map();
  for (const pkg of metadata.packages ?? []) {
    for (const t of pkg.targets ?? []) {
      const kind = t.kind?.[0];
      const src = t.src_path;
      if (kind === "lib" || kind === "proc-macro" || kind === "rlib" || kind === "cdylib" || kind === "dylib" || kind === "staticlib") {
        byId.set(pkg.name, src);
      } else if (kind === "test") {
        byId.set(`${pkg.name}::${t.name}`, src);
      } else if (kind === "bin" || kind === "bench" || kind === "example") {
        byId.set(`${pkg.name}::${kind}/${t.name}`, src);
      }
    }
  }
  return byId;
}

/**
 * El archivo más profundo que existe para el módulo de un test unitario. `a::b::tests::x` busca
 * `a/b/tests.rs`, `a/b/tests/mod.rs`, `a/b.rs`, `a/b/mod.rs`, `a.rs`, `a/mod.rs` y cae en la raíz del
 * crate; un `mod tests {}` en línea vive en el archivo padre, por eso se baja prefijo a prefijo.
 */
export function unitTestFile(rootFile, testName, exists = existsSync) {
  const modules = testName.split("::").slice(0, -1);
  const base = dirname(rootFile);
  for (let n = modules.length; n > 0; n--) {
    const stem = join(base, ...modules.slice(0, n));
    for (const candidate of [`${stem}.rs`, join(stem, "mod.rs")]) {
      if (exists(candidate)) return candidate;
    }
  }
  return rootFile;
}

/** Archivo fuente de un test, o null si el binario no se conoce. */
export function resolveTestFile(binaryId, testName, index, exists = existsSync) {
  const root = index.get(binaryId);
  if (!root) return null;
  // Los tests unitarios (lib o bin) viven en los módulos del crate; integración, benches y
  // ejemplos se asignan a su archivo raíz.
  const unit = !binaryId.includes("::") || binaryId.includes("::bin/");
  return unit ? unitTestFile(root, testName, exists) : root;
}

const decodeXml = (s) => s.replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const attr = (tag, name) => {
  const m = new RegExp(`\\s${name}="([^"]*)"`).exec(tag);
  return m ? decodeXml(m[1]) : undefined;
};
const escapeAttr = (s) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Cuenta de un <testcase> completo: falla (`<failure>`), error (`<error>`), omitido (`<skipped>`). */
function outcome(testcaseXml) {
  if (/<failure\b/.test(testcaseXml)) return "failures";
  if (/<error\b/.test(testcaseXml)) return "errors";
  if (/<skipped\b/.test(testcaseXml)) return "skipped";
  return "passed";
}

/**
 * Reescribe el JUnit de nextest con un <testsuite> por archivo fuente. Devuelve `{ xml, unresolved }`;
 * los tests que no se resuelven conservan el nombre de suite original.
 */
export function rewriteJunit(xml, index, repoRoot, exists = existsSync) {
  const head = /<testsuites\b[^>]*>/.exec(xml)?.[0] ?? '<testsuites name="nextest-run">';
  const groups = new Map();
  const unresolved = new Set();
  const suiteRe = /<testsuite\b([^>]*?)(?:\/>|>([\s\S]*?)<\/testsuite>)/g;
  for (const s of xml.matchAll(suiteRe)) {
    const binaryId = attr(s[1], "name") ?? "";
    const body = s[2] ?? "";
    for (const c of body.matchAll(/<testcase\b[^>]*?(?:\/>|>[\s\S]*?<\/testcase>)/g)) {
      const testName = attr(c[0].slice(0, c[0].indexOf(">") + 1), "name") ?? "";
      const file = resolveTestFile(binaryId, testName, index, exists);
      let key = binaryId;
      if (file) {
        const rel = posix(relative(repoRoot, file));
        key = rel.startsWith("..") || isAbsolute(rel) ? posix(file) : rel;
      } else {
        unresolved.add(binaryId);
      }
      if (!groups.has(key)) groups.set(key, { cases: [], tests: 0, skipped: 0, errors: 0, failures: 0 });
      const g = groups.get(key);
      g.cases.push(c[0]);
      g.tests++;
      const o = outcome(c[0]);
      if (o !== "passed") g[o]++;
    }
  }
  const lines = ['<?xml version="1.0" encoding="UTF-8"?>', head];
  for (const [name, g] of groups) {
    lines.push(`    <testsuite name="${escapeAttr(name)}" tests="${g.tests}" skipped="${g.skipped}" errors="${g.errors}" failures="${g.failures}">`);
    for (const c of g.cases) lines.push(`        ${c.trim()}`);
    lines.push("    </testsuite>");
  }
  lines.push("</testsuites>", "");
  return { xml: lines.join("\n"), unresolved: [...unresolved] };
}

function cargoMetadata(cwd) {
  const r = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { cwd, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`cargo metadata falló: ${(r.stderr || r.error?.message || "").trim()}`);
  return JSON.parse(r.stdout);
}

/** Corre nextest y escribe `junit-paths.xml`; devuelve el código de salida de nextest. */
export function run(args, cwd = process.cwd()) {
  let metadata = null;
  try {
    metadata = cargoMetadata(cwd);
  } catch (e) {
    console.error(`nextest-junit: ${e.message}`);
  }
  const outDir = join(metadata?.target_directory ?? resolve(cwd, "target"), "nextest", "ci");
  const source = join(outDir, "junit.xml");
  const target = join(outDir, "junit-paths.xml");
  // Un informe de una corrida anterior diría lo que esta no dijo.
  rmSync(source, { force: true });
  rmSync(target, { force: true });

  const r = spawnSync("cargo", ["nextest", "run", "--profile", "ci", ...(args.length ? args : ["--workspace"])], { cwd, stdio: "inherit" });
  const code = r.status ?? (r.error ? 127 : 1);
  if (r.error) console.error(`nextest-junit: no se pudo ejecutar cargo nextest: ${r.error.message}`);

  if (!existsSync(source)) {
    console.error(`nextest-junit: nextest no escribió ${source}; no hay informe que adaptar`);
    return code;
  }
  if (!metadata) {
    console.error("nextest-junit: sin cargo metadata no se puede resolver ningún archivo; no se escribe junit-paths.xml");
    return code;
  }
  try {
    const { xml, unresolved } = rewriteJunit(readFileSync(source, "utf8"), buildIndex(metadata), metadata.workspace_root);
    for (const id of unresolved) console.error(`nextest-junit: no se resolvió el archivo del binario \`${id}\`; se deja su nombre original`);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, xml);
  } catch (e) {
    console.error(`nextest-junit: no se pudo escribir ${target}: ${e.message}`);
  }
  return code;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exit(run(process.argv.slice(2)));
}
