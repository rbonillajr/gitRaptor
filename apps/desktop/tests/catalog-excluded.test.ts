// TS003-CATALOG-EXCLUDED (TS-DSK-003 T009, D7): the production build contains no trace of the dev catalog.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build } from "vite";
import { describe, expect, it } from "vitest";

const root = new URL("..", import.meta.url).pathname;
const catalogFile = join(root, "src", "dev", "catalog", "Catalog.tsx");

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

/** Positive control: without the catalog source the absence in dist proves nothing. */
function controlSource(): string {
  expect(existsSync(catalogFile), "src/dev/catalog/Catalog.tsx does not exist: the catalog is not built yet").toBe(true);
  return readFileSync(catalogFile, "utf8");
}

describe("TS003-CATALOG-EXCLUDED release build", () => {
  it("TS003-CATALOG-EXCLUDED: the catalog source carries the sentinel (positive control)", () => {
    expect(controlSource(), "Catalog.tsx must contain gr-dev-catalog").toContain("gr-dev-catalog");
  });

  it("TS003-CATALOG-EXCLUDED: no file of the production build has the sentinel or a name with catalog", async () => {
    controlSource();
    const outDir = await mkdtemp(join(tmpdir(), "gr-dist-"));
    try {
      await build({
        configFile: join(root, "vite.config.ts"),
        root,
        mode: "production",
        logLevel: "silent",
        build: { outDir, emptyOutDir: true },
      });
      const files = walk(outDir);
      expect(files.length, "the build produced no files").toBeGreaterThan(0);
      const names = files.filter((f) => /catalog/i.test(f.slice(outDir.length)));
      expect(names, "a build file has `catalog` in its name").toEqual([]);
      const leaks = files.filter((f) => /gr-dev-catalog|data-gr-dev-catalog/.test(readFileSync(f, "utf8")));
      expect(leaks, "a build file contains the gr-dev-catalog sentinel").toEqual([]);
    } finally {
      await rm(outDir, { recursive: true, force: true });
    }
  }, 180_000);
});
