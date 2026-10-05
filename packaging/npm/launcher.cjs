#!/usr/bin/env node
// GitRaptor npm launcher (INF-GRP-004, ADR-GRP-014 § 5). Runs the native binary shipped by the
// platform package that npm installed through optionalDependencies. It never downloads anything:
// npm already verified the package integrity, and `raptor-mcp` started by an agent must not open
// the network or wait on a download.
"use strict";

const { spawnSync } = require("node:child_process");

const BIN = "__BIN__";
const PLATFORMS = __PLATFORMS__;

const key = `${process.platform}-${process.arch}`;
const pkg = PLATFORMS[key];
if (!pkg) {
  console.error(`gitraptor: no prebuilt binary for ${key}`);
  process.exit(1);
}

const exe = process.platform === "win32" ? `${BIN}.exe` : BIN;
let binary;
try {
  binary = require.resolve(`${pkg}/bin/${exe}`);
} catch {
  console.error(
    `gitraptor: the platform package ${pkg} is not installed. ` +
      "Reinstall without --no-optional / --omit=optional.",
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (result.error) {
  console.error(`gitraptor: could not run ${binary}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
