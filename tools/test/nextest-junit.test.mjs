// node --test tools/test/nextest-junit.test.mjs
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { buildIndex, rewriteJunit, unitTestFile } from "./nextest-junit.mjs";

const SCRIPT = fileURLToPath(new URL("./nextest-junit.mjs", import.meta.url));

/** Un workspace de mentira con un crate `demo-crate` y sus archivos fuente vacíos. */
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "nextest-junit-"));
  for (const f of ["src/lib.rs", "src/util/mod.rs", "src/util/tests.rs", "src/store.rs", "tests/us_x.rs", "benches/b1.rs", "src/main.rs"]) {
    mkdirSync(join(root, f, ".."), { recursive: true });
    writeFileSync(join(root, f), "");
  }
  const metadata = {
    workspace_root: root,
    target_directory: join(root, "target"),
    packages: [
      {
        name: "demo-crate",
        targets: [
          { kind: ["lib"], name: "demo_crate", src_path: join(root, "src/lib.rs") },
          { kind: ["bin"], name: "demo-cli", src_path: join(root, "src/main.rs") },
          { kind: ["test"], name: "us_x", src_path: join(root, "tests/us_x.rs") },
          { kind: ["bench"], name: "b1", src_path: join(root, "benches/b1.rs") },
        ],
      },
    ],
  };
  return { root, metadata };
}

const case_ = (suite, name, failed = false) =>
  failed
    ? `        <testcase name="${name}" classname="${suite}" time="0.01">\n            <failure message="boom &lt;&amp;&gt;" type="test failure">trace</failure>\n            <system-out>a &lt;b&gt;</system-out>\n        </testcase>`
    : `        <testcase name="${name}" classname="${suite}" time="0.01"/>`;

const JUNIT = [
  '<?xml version="1.0" encoding="UTF-8"?>',
  '<testsuites name="nextest-run" tests="8" skipped="1" failures="2" errors="0">',
  '    <testsuite name="demo-crate" tests="4" skipped="0" errors="0" failures="1">',
  case_("demo-crate", "tests::inline_ok"),
  case_("demo-crate", "util::tests::own_file_red", true),
  case_("demo-crate", "store::tests::roundtrip"),
  case_("demo-crate", "missing::module::case"),
  "    </testsuite>",
  '    <testsuite name="demo-crate::us_x" tests="2" skipped="0" errors="0" failures="1">',
  case_("demo-crate::us_x", "integ_red", true),
  case_("demo-crate::us_x", "integ_ok"),
  "    </testsuite>",
  '    <testsuite name="demo-crate::bench/b1" tests="1" skipped="1" errors="0" failures="0">',
  '        <testcase name="b" classname="demo-crate::bench/b1"><skipped/></testcase>',
  "    </testsuite>",
  '    <testsuite name="demo-crate::bin/demo-cli" tests="1" skipped="0" errors="0" failures="0">',
  case_("demo-crate::bin/demo-cli", "cli::parses"),
  "    </testsuite>",
  "</testsuites>",
  "",
].join("\n");

const suites = (xml) => [...xml.matchAll(/<testsuite name="([^"]*)" tests="(\d+)" skipped="(\d+)" errors="(\d+)" failures="(\d+)">/g)].map((m) => ({ name: m[1], tests: +m[2], skipped: +m[3], errors: +m[4], failures: +m[5] }));

test("integration tests map to the src_path of their test target", () => {
  const { root, metadata } = fixture();
  const { xml } = rewriteJunit(JUNIT, buildIndex(metadata), root);
  const s = suites(xml).find((x) => x.name === "tests/us_x.rs");
  assert.deepEqual(s, { name: "tests/us_x.rs", tests: 2, skipped: 0, errors: 0, failures: 1 });
});

test("a unit test in its own file maps to that file", () => {
  const { root, metadata } = fixture();
  const { xml } = rewriteJunit(JUNIT, buildIndex(metadata), root);
  const s = suites(xml).find((x) => x.name === "src/util/tests.rs");
  assert.deepEqual(s, { name: "src/util/tests.rs", tests: 1, skipped: 0, errors: 0, failures: 1 });
});

test("an inline `mod tests` maps to the parent file, and the deepest existing prefix wins", () => {
  const { root, metadata } = fixture();
  const { xml } = rewriteJunit(JUNIT, buildIndex(metadata), root);
  const names = suites(xml).map((x) => x.name);
  assert.ok(names.includes("src/store.rs"));
  assert.ok(names.includes("src/lib.rs"));
  assert.equal(unitTestFile("/c/src/lib.rs", "a::b::tests::x", (p) => p === "/c/src/a/b.rs" || p === "/c/src/a.rs"), "/c/src/a/b.rs");
  assert.equal(unitTestFile("/c/src/lib.rs", "a::b::tests::x", (p) => p === "/c/src/a/b/mod.rs"), "/c/src/a/b/mod.rs");
  assert.equal(unitTestFile("/c/src/lib.rs", "a::b::tests::x", (p) => p === "/c/src/a/b/tests/mod.rs"), "/c/src/a/b/tests/mod.rs");
  assert.equal(unitTestFile("/c/src/lib.rs", "x", () => false), "/c/src/lib.rs");
});

test("benches and binaries map to their src_path", () => {
  const { root, metadata } = fixture();
  const { xml } = rewriteJunit(JUNIT, buildIndex(metadata), root);
  const names = suites(xml).map((x) => x.name);
  assert.ok(names.includes("benches/b1.rs"));
  assert.ok(names.includes("src/main.rs"));
});

test("a red test keeps its failure body and counters add up with no testcase lost", () => {
  const { root, metadata } = fixture();
  const { xml } = rewriteJunit(JUNIT, buildIndex(metadata), root);
  const all = suites(xml);
  const sum = (k) => all.reduce((a, s) => a + s[k], 0);
  assert.equal(sum("tests"), 8);
  assert.equal(sum("failures"), 2);
  assert.equal(sum("skipped"), 1);
  assert.equal(sum("errors"), 0);
  assert.equal((xml.match(/<testcase\b/g) ?? []).length, 8);
  assert.equal((xml.match(/<failure\b/g) ?? []).length, 2);
  assert.ok(xml.includes('<failure message="boom &lt;&amp;&gt;" type="test failure">trace</failure>'));
  assert.ok(xml.includes("<system-out>a &lt;b&gt;</system-out>"));
});

test("an unresolved binary keeps its name and is reported", () => {
  const { root, metadata } = fixture();
  const { xml, unresolved } = rewriteJunit(JUNIT, buildIndex(metadata), root);
  // `missing::module::case` resolves to lib.rs (unit fallback); an unknown binary does not resolve.
  assert.deepEqual(unresolved, []);
  const odd = JUNIT.replace(/demo-crate::bin\/demo-cli/g, "ghost::bin/ghost");
  const r = rewriteJunit(odd, buildIndex(metadata), root);
  assert.deepEqual(r.unresolved, ["ghost::bin/ghost"]);
  assert.ok(r.xml.includes('<testsuite name="ghost::bin/ghost" tests="1"'));
  assert.ok(xml.length > 0);
});

/** Un `cargo` falso en PATH: responde a `metadata` y a `nextest run`, escribe el JUnit y sale con `FAKE_CODE`. */
function fakeCargo(root, metadata) {
  const bin = join(root, "fakebin");
  mkdirSync(bin, { recursive: true });
  const script = `#!/usr/bin/env node
const fs = require("fs");
const a = process.argv.slice(2);
if (a[0] === "metadata") { process.stdout.write(${JSON.stringify(JSON.stringify(metadata))}); process.exit(0); }
fs.writeFileSync(${JSON.stringify(join(root, "args.txt"))}, a.join(" "));
if (process.env.FAKE_NO_REPORT !== "1") {
  fs.mkdirSync(${JSON.stringify(join(root, "target/nextest/ci"))}, { recursive: true });
  fs.writeFileSync(${JSON.stringify(join(root, "target/nextest/ci/junit.xml"))}, fs.readFileSync(${JSON.stringify(join(root, "sample.xml"))}, "utf8"));
}
process.exit(Number(process.env.FAKE_CODE || 0));
`;
  writeFileSync(join(bin, "cargo"), script);
  chmodSync(join(bin, "cargo"), 0o755);
  writeFileSync(join(root, "sample.xml"), JUNIT);
  return bin;
}

const unix = process.platform !== "win32";

test("the exit code of nextest is propagated and the adapted report is written", { skip: !unix }, () => {
  const { root, metadata } = fixture();
  const bin = fakeCargo(root, metadata);
  for (const code of [0, 100]) {
    const r = spawnSync(process.execPath, [SCRIPT], { cwd: root, encoding: "utf8", env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FAKE_CODE: String(code) } });
    assert.equal(r.status, code, r.stderr);
    const out = readFileSync(join(root, "target/nextest/ci/junit-paths.xml"), "utf8");
    assert.ok(out.includes('<testsuite name="tests/us_x.rs"'));
  }
  assert.equal(readFileSync(join(root, "args.txt"), "utf8"), "nextest run --profile ci --workspace");
});

test("arguments are forwarded and a missing report still returns nextest's code", { skip: !unix }, () => {
  const { root, metadata } = fixture();
  const bin = fakeCargo(root, metadata);
  const r = spawnSync(process.execPath, [SCRIPT, "-p", "demo-crate"], { cwd: root, encoding: "utf8", env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FAKE_CODE: "101", FAKE_NO_REPORT: "1" } });
  assert.equal(r.status, 101);
  assert.match(r.stderr, /no hay informe que adaptar/);
  assert.equal(readFileSync(join(root, "args.txt"), "utf8"), "nextest run --profile ci -p demo-crate");
});
