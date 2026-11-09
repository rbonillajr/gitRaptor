import type { Environment } from "vitest/environments";
import { builtinEnvironments } from "vitest/environments";

/**
 * jsdom with the modules transformed for the server. In the `client` transform, Vite rewrites
 * `new URL("…", import.meta.url)` into an asset URL of the dev server, so tests that read repo files
 * (`commands.test.ts`, `eslint-v5.test.ts`) would get `http://localhost:3000/` instead of a file URL.
 */
const environment: Environment = { ...builtinEnvironments.jsdom, name: "jsdom-node-urls", viteEnvironment: "ssr" };

export default environment;
