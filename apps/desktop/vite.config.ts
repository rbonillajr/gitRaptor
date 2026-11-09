/// <reference types="vitest/config" />
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

const STUB_ID = "\0gr-dev-stub";

/** The release build never contains the dev catalog: its dynamic import resolves to an empty module (DS-TS-DSK-003 D7). */
const stripDevCatalog: Plugin = {
  name: "gr-dev-catalog-strip",
  apply: "build",
  enforce: "pre",
  resolveId(id) {
    return /(^|\/)dev\/catalog\/Catalog$/.test(id) ? STUB_ID : null;
  },
  load(id) {
    return id === STUB_ID ? "export const Catalog = () => null;" : null;
  },
};

export default defineConfig({
  plugins: [react(), tailwindcss(), stripDevCatalog],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "localhost" },
  build: { target: "safari16", outDir: "dist", sourcemap: false },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  test: {
    environment: "./tests/jsdom-node-urls.ts",
    globals: true,
    setupFiles: ["tests/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}", "tests/**/*.test.ts"],
  },
});
