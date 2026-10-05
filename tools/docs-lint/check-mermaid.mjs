// docs-lint: renders every ```mermaid block under docs/**/*.md with the Mermaid CLI and fails
// if any block does not render. Usage: node check-mermaid.mjs [root-dir] (default: ../../docs).
//
// On GitHub Actions each failure is also reported as an `::error` annotation on file and line.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { renderMermaid } from "@mermaid-js/mermaid-cli";
import puppeteer from "puppeteer";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const root = path.resolve(process.argv[2] ?? path.join(repoRoot, "docs"));

/** Returns the Mermaid blocks of a Markdown file as { line, definition } (line is 1-based). */
function extractMermaidBlocks(markdown) {
  const blocks = [];
  const lines = markdown.split(/\r?\n/);
  let open = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (open === null) {
      const m = /^(\s*)(`{3,}|~{3,})\s*mermaid\s*$/.exec(line);
      if (m) open = { fence: m[2], line: i + 1, body: [] };
    } else if (line.trim().startsWith(open.fence) && line.trim().replace(/[`~]/g, "") === "") {
      blocks.push({ line: open.line, definition: open.body.join("\n") });
      open = null;
    } else {
      open.body.push(line);
    }
  }
  if (open !== null) blocks.push({ line: open.line, definition: null });
  return blocks;
}

function markdownFiles(dir) {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith(".md"))
    .map((e) => path.join(e.parentPath, e.name))
    .filter((f) => !f.split(path.sep).includes("node_modules"))
    .sort();
}

function firstLine(message) {
  return String(message).split("\n").find((l) => l.trim() !== "") ?? String(message);
}

async function main() {
  const files = markdownFiles(root);
  const targets = files.flatMap((file) =>
    extractMermaidBlocks(readFileSync(file, "utf8")).map((b) => ({ file, ...b })),
  );
  // Hosted Ubuntu runners restrict unprivileged user namespaces (AppArmor), so Chrome's sandbox
  // cannot start there; the input is this repo's own docs, never untrusted content.
  const browser = await puppeteer.launch({ headless: true, args: ["--no-sandbox"] });
  const failures = [];
  try {
    for (const t of targets) {
      const rel = path.relative(repoRoot, t.file);
      if (t.definition === null) {
        failures.push({ rel, line: t.line, message: "unterminated mermaid fence" });
        continue;
      }
      try {
        await renderMermaid(browser, t.definition, "svg");
        console.log(`ok   ${rel}:${t.line}`);
      } catch (err) {
        failures.push({ rel, line: t.line, message: firstLine(err?.message ?? err) });
      }
    }
  } finally {
    await browser.close();
  }
  for (const f of failures) {
    console.log(`FAIL ${f.rel}:${f.line}: ${f.message}`);
    if (process.env.GITHUB_ACTIONS === "true") {
      console.log(`::error file=${f.rel},line=${f.line}::Mermaid block does not render: ${f.message}`);
    }
  }
  console.log(
    `${targets.length} Mermaid block(s) in ${files.length} file(s); ${failures.length} failed.`,
  );
  if (targets.length === 0) {
    console.log("No Mermaid blocks found: check the root directory.");
    process.exitCode = 1;
  }
  if (failures.length > 0) process.exitCode = 1;
}

await main();
