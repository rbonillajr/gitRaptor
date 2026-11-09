// D7-GATE (DS-US-DSK-007 T006, D6, D10, ADR-GRP-017 § 5): the app never reads the clipboard and only CopyCommand writes
// it, so the exception command can be copied by the person but nothing the app does can paste or run it.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const SRC = fileURLToPath(new URL("..", import.meta.url));

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}
/** Every production source of `src` (tests excluded), as `[path relative to src, text]`. */
const sources = walk(SRC)
  .filter((p) => /\.(ts|tsx)$/.test(p) && !/\.test\.(ts|tsx)$/.test(p))
  .map((p) => [relative(SRC, p), readFileSync(p, "utf8")] as const);
const read = (file: string) => readFileSync(join(SRC, file), "utf8");

describe("D7-GATE clipboard boundaries", () => {
  it("D7-GATE: the scan sees the sources (positive control)", () => {
    expect(sources.length, "src has production files").toBeGreaterThan(20);
    expect(sources.map(([p]) => p), "CopyCommand is where the clipboard is written").toContain(join("ui", "CopyCommand.tsx"));
  });

  it("D7-GATE: the app never reads the clipboard", () => {
    const readers = sources.filter(([, text]) => /clipboard\.read|readText|navigator\.clipboard\.read/.test(text)).map(([p]) => p);
    expect(readers, "no file of src reads the clipboard (ADR-GRP-017 R-5b)").toEqual([]);
  });

  it("D7-GATE: only CopyCommand writes it", () => {
    const writers = sources.filter(([, text]) => /navigator\.clipboard\.writeText|clipboard\.writeText|\bwriteText\b/.test(text)).map(([p]) => p);
    expect(writers, "writeText appears only in src/ui/CopyCommand.tsx").toEqual([join("ui", "CopyCommand.tsx")]);
  });

  it("D7-GATE: the block dialog and the announced exception copy only through CopyCommand", () => {
    for (const file of ["guardrails/BlockDialog.tsx", "guardrails/PendingException.tsx"]) {
      const text = read(file);
      expect(text, `${file} must use CopyCommand for the command`).toContain("CopyCommand");
      expect(text, `${file} must not touch the clipboard itself`).not.toMatch(/clipboard|writeText/);
    }
  });

  it("D7-GATE: no file of the Guardrails view sends an exception, cancel or write method to the engine", () => {
    const files = sources.filter(([p]) => p.startsWith("guardrails")).map(([, text]) => text).join("\n");
    expect(files.length, "the Guardrails sources exist and are not stubs").toBeGreaterThan(3_000);
    expect(files, "the app never asks the engine for an exception").not.toMatch(/guard\.exception|guard\.cancel|guard\.install|guard\.uninstall|--yes/);
  });
});
