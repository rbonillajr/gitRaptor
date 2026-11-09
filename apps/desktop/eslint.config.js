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
    rules: restrictImports(BASE_IMPORTS, TAURI_IMPORTS, CLIENT_IMPORTS),
  },
);
