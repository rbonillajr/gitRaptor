// ESLint flat config of @gitraptor/desktop. The V5 rules (ADR-GRP-017, DS-TS-DSK-001 § Firmas del
// stack) forbid untrusted-HTML sinks, markdown libraries and Tauri plugins everywhere, @tauri-apps/api
// outside src/client and the client layer inside src/ui.
import js from "@eslint/js";
import { defineConfig } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

const HTML_SINKS = [
  { selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']", message: "No HTML from text (V5, ADR-GRP-017)." },
  {
    selector: "AssignmentExpression > MemberExpression.left[property.name=/^(innerHTML|outerHTML)$/]",
    message: "No HTML from text (V5, ADR-GRP-017).",
  },
  { selector: "CallExpression[callee.property.name='insertAdjacentHTML']", message: "No HTML from text (V5, ADR-GRP-017)." },
  {
    selector: "CallExpression[callee.object.name='document'][callee.property.name='write']",
    message: "No HTML from text (V5, ADR-GRP-017).",
  },
  // Beyond the Dev Spec's list (security review L-03, 2026-11-08): the other parsers of HTML strings.
  { selector: "JSXAttribute[name.name='srcDoc']", message: "No HTML from text (V5, ADR-GRP-017)." },
  {
    selector: "CallExpression[callee.property.name=/^(setHTMLUnsafe|createContextualFragment|parseFromString)$/]",
    message: "No HTML from text (V5, ADR-GRP-017).",
  },
];

const BASE_IMPORTS = {
  group: [
    "react-markdown",
    "marked",
    "markdown-it",
    "remark*",
    "rehype*",
    "micromark",
    "showdown",
    "dompurify",
    "@tauri-apps/plugin-*",
  ],
  message: "Markdown/HTML libraries and Tauri plugins are not allowed (V5, ADR-GRP-017).",
};
const TAURI_API = ["@tauri-apps/api", "@tauri-apps/api/*"];
const TAURI_IMPORTS = { group: TAURI_API, message: "Only src/client talks to Tauri (V5, ADR-GRP-017)." };
/** Tests outside src/client may mock the IPC, never call it. */
const TAURI_IMPORTS_IN_TESTS = { regex: "^@tauri-apps/api(/(?!mocks$).*)?$", message: TAURI_IMPORTS.message };
const CLIENT_IMPORTS = {
  group: ["@/client", "@/client/*", "**/client/**"],
  message: "src/ui holds pure components: no client layer (V5).",
};
const UI_PURITY_IMPORTS = {
  group: ["@/state", "@/state/*", "@/shell", "@/shell/*", "@/dev", "@/dev/*", "@/app/*"],
  message: "src/ui holds pure components: no state, shell, app or dev imports (TS-DSK-003).",
};
const DEV_IMPORTS = {
  group: ["@/dev", "@/dev/*"],
  message: "The dev catalog is reachable only from main.tsx under import.meta.env.DEV (TS-DSK-003)",
};

const restrictImports = (...patterns) => ({ "no-restricted-imports": ["error", { patterns }] });
const TESTS = ["**/*.test.ts", "**/*.test.tsx"];

export default defineConfig(
  { ignores: ["dist", "src-tauri", "tests/fixtures"] },
  js.configs.recommended,
  tseslint.configs.strict,
  {
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
    plugins: { "react-hooks": reactHooks },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      "no-restricted-syntax": ["error", ...HTML_SINKS],
      ...restrictImports(BASE_IMPORTS),
    },
  },
  {
    // Everything outside src/client: src, tests, configs and any new folder.
    ignores: ["src/client/**", ...TESTS, "tests/**"],
    rules: restrictImports(BASE_IMPORTS, TAURI_IMPORTS),
  },
  {
    files: [...TESTS, "tests/**"],
    ignores: ["src/client/**"],
    rules: restrictImports(BASE_IMPORTS, TAURI_IMPORTS_IN_TESTS),
  },
  {
    files: ["src/ui/**"],
    rules: restrictImports(BASE_IMPORTS, TAURI_IMPORTS, CLIENT_IMPORTS, UI_PURITY_IMPORTS),
  },
  {
    // The dev catalog is imported only by main.tsx (and by itself).
    files: ["src/**"],
    ignores: ["src/main.tsx", "src/dev/**", "src/client/**", "src/ui/**", ...TESTS],
    rules: restrictImports(BASE_IMPORTS, TAURI_IMPORTS, DEV_IMPORTS),
  },
  {
    files: ["src/client/**"],
    ignores: [...TESTS],
    rules: restrictImports(BASE_IMPORTS, DEV_IMPORTS),
  },
);
