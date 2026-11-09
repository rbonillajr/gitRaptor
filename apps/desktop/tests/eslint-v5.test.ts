// V5 (TS-DSK-001 T005, ADR-GRP-017): ESLint forbids untrusted-HTML sinks, markdown libraries, the Tauri
// plugins everywhere in the frontend and @tauri-apps/api outside src/client.
// Assumes apps/desktop/eslint.config.js (flat config) exists; files are virtual (lintText).
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

const cwd = new URL("..", import.meta.url).pathname;
const eslint = new ESLint({ cwd });

async function errors(code: string, filePath: string): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath: `${cwd}${filePath}` });
  return (result?.messages ?? []).filter((m) => m.severity === 2).map((m) => `${m.ruleId ?? "parse"}: ${m.message}`);
}

describe("V5 untrusted HTML and imports", () => {
  const ui = "src/ui/x.tsx";

  it("V5: dangerouslySetInnerHTML is an error", async () => {
    const code = "export const X = ({ s }: { s: string }) => <div dangerouslySetInnerHTML={{ __html: s }} />;\n";
    expect(await errors(code, ui), "dangerouslySetInnerHTML must be rejected").not.toEqual([]);
  });

  it("V5: innerHTML assignment is an error", async () => {
    const code = "export function f(el: HTMLElement, s: string) {\n  el.innerHTML = s;\n}\n";
    expect(await errors(code, "src/shell/x.ts"), "el.innerHTML = s must be rejected").not.toEqual([]);
  });

  it("V5: outerHTML assignment is an error", async () => {
    const code = "export function f(el: HTMLElement, s: string) {\n  el.outerHTML = s;\n}\n";
    expect(await errors(code, "src/shell/x.ts"), "el.outerHTML = s must be rejected").not.toEqual([]);
  });

  it("V5: insertAdjacentHTML is an error", async () => {
    const code = 'export function f(el: HTMLElement, s: string) {\n  el.insertAdjacentHTML("beforeend", s);\n}\n';
    expect(await errors(code, "src/shell/x.ts"), "insertAdjacentHTML must be rejected").not.toEqual([]);
  });

  it("V5: document.write is an error", async () => {
    const code = "export function f(s: string) {\n  document.write(s);\n}\n";
    expect(await errors(code, "src/shell/x.ts"), "document.write must be rejected").not.toEqual([]);
  });

  it.each(["react-markdown", "marked", "markdown-it", "remark-gfm", "rehype-raw", "micromark", "showdown", "dompurify"])(
    "V5: importing %s is an error",
    async (pkg) => {
      const code = `import x from "${pkg}";\nexport const y = x;\n`;
      expect(await errors(code, "src/shell/x.tsx"), `${pkg} must be rejected`).not.toEqual([]);
    },
  );

  it("V5: Tauri plugins are an error everywhere, including src/client", async () => {
    for (const file of ["src/client/x.ts", "src/shell/x.tsx", "src/ui/x.tsx"]) {
      const code = 'import { open } from "@tauri-apps/plugin-shell";\nexport const y = open;\n';
      expect(await errors(code, file), `@tauri-apps/plugin-shell in ${file} must be rejected`).not.toEqual([]);
    }
  });

  it("V5: @tauri-apps/api is an error outside src/client", async () => {
    const core = 'import { invoke } from "@tauri-apps/api/core";\nexport const y = invoke;\n';
    expect(await errors(core, ui), "@tauri-apps/api/core in src/ui").not.toEqual([]);
    expect(await errors(core, "src/shell/x.tsx"), "@tauri-apps/api/core in src/shell").not.toEqual([]);
    const app = 'import { getVersion } from "@tauri-apps/api/app";\nexport const y = getVersion;\n';
    expect(await errors(app, "src/state/x.ts"), "@tauri-apps/api/app in src/state").not.toEqual([]);
  });

  it("V5: @/client is an error in src/ui", async () => {
    const code = 'import { x } from "@/client/ipc";\nexport const y = x;\n';
    expect(await errors(code, ui), "@/client/ipc in src/ui must be rejected").not.toEqual([]);
  });

  it("V5: @tauri-apps/api/core is accepted in src/client", async () => {
    const code = 'import { invoke } from "@tauri-apps/api/core";\nexport const y = invoke;\n';
    expect(await errors(code, "src/client/x.ts"), "src/client may import @tauri-apps/api").toEqual([]);
  });
});

describe("TS003-BOUNDARIES src/ui purity and the dev catalog", () => {
  it("TS003-BOUNDARIES: @/state is an error in src/ui", async () => {
    const code = 'import { useUiStore } from "@/state/ui-store";\nexport const y = useUiStore;\n';
    expect(await errors(code, "src/ui/x.tsx"), "@/state in src/ui must be rejected").not.toEqual([]);
  });

  it("TS003-BOUNDARIES: @/shell, @/dev and @/app are errors in src/ui", async () => {
    for (const spec of ["@/shell/TitleBar", "@/dev/catalog/Catalog", "@/app/App"]) {
      const code = `import x from "${spec}";\nexport const y = x;\n`;
      expect(await errors(code, "src/ui/x.tsx"), `${spec} in src/ui must be rejected`).not.toEqual([]);
    }
  });

  it("TS003-BOUNDARIES: only main.tsx may import @/dev", async () => {
    const code = 'import "@/dev/catalog/Catalog";\n';
    expect(await errors(code, "src/shell/x.tsx"), "@/dev in src/shell must be rejected").not.toEqual([]);
    expect(await errors(code, "src/state/x.ts"), "@/dev in src/state must be rejected").not.toEqual([]);
    expect(await errors(code, "src/main.tsx"), "src/main.tsx may import the dev catalog").toEqual([]);
  });
});
