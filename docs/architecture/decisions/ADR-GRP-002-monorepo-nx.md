---
id: ADR-GRP-002
title: Monorepo políglota con Nx (package-based), pnpm y Cargo workspaces
type: adr
status: accepted
date: 2026-10-01
created: 2026-10-01
updated: 2026-10-01
deciders: [Rene Bonilla]
related: [ADR-GRP-001, ADR-GRP-003]
tags: [nx, monorepo, package-based, pnpm, cargo, rust, monodon, tauri, ci]
---

# ADR-GRP-002 — Monorepo con Nx (package-based)

## Contexto

GitRaptor es políglota (ADR-GRP-001):
- Rust para el motor, la CLI/TUI, el MCP y el backend de Tauri.
- TypeScript/React para la app de escritorio, el UI kit y la extensión de VS Code/Cursor.

Necesitamos:
- Compartir código entre apps, como el UI kit (ADR-GRP-003) y los tipos del motor.
- Builds y tests incrementales con caché.
- Un grafo de dependencias entre proyectos.
- Releases coordinados de binarios, VSIX y crates.

## Decisión

Usar **Nx en modo package-based**:

- **JS/TS:** **pnpm workspaces**. Cada paquete tiene su `package.json` con sus propios scripts, que Nx infiere como targets. Las dependencias internas van con `workspace:*`.
- **Rust:** un **Cargo workspace** en la raíz (`Cargo.toml` con `[workspace]`, `members` y `[workspace.dependencies]`), integrado a Nx con el plugin **`@monodon/rust`** (v3.x). El plugin expone `build`, `test`, `lint` (clippy) y `run`, y construye el grafo a partir de las dependencias `path`.
- **Tauri:** el frontend es un paquete normal de pnpm. `src-tauri` es miembro del Cargo workspace. `tauri dev` y `tauri build` corren como scripts de `package.json`.
- **Caché:** activada en `nx.json` (`targetDefaults` con `cache: true` para build, test y lint). Un `namedInput` `rust` cubre `Cargo.toml`, `Cargo.lock` y `rust-toolchain.toml`.
- **CI:** `nx affected` para correr solo lo que cambió. Nx Cloud o una caché remota son opcionales.
- **Fronteras:** tags de Nx (`scope:*`, `type:*`) y la regla `@nx/enforce-module-boundaries` para evitar dependencias indebidas. Por ejemplo, `ui-kit` no puede depender de apps.

## Estructura propuesta

```
gitraptor/
├─ nx.json
├─ package.json            # scripts raíz, devDeps (nx, @monodon/rust, typescript)
├─ pnpm-workspace.yaml     # apps/*, packages/*
├─ Cargo.toml              # [workspace] members = ["crates/*", "apps/*/src-tauri", "apps/cli", "apps/mcp"]
├─ rust-toolchain.toml
├─ apps/
│  ├─ cli/                 # Rust · binario `raptor` (CLI + TUI con ratatui)
│  ├─ mcp/                 # Rust · binario `raptor-mcp` (rmcp)
│  ├─ desktop/             # React + Vite (frontend Tauri)
│  │  └─ src-tauri/        # Rust · backend Tauri → usa crates/core
│  └─ vscode-extension/    # TS · cliente liviano del motor (Fase 3)
├─ crates/
│  ├─ core/                # Rust · motor: watcher, oplog/snapshots, conflictos
│  ├─ policy/              # Rust · motor de guardrails (policy.yaml)
│  ├─ git/                 # Rust · capa Git (gitoxide para leer + Git CLI para escribir)
│  ├─ api/                 # Rust · contrato JSON-RPC/eventos; genera tipos TS (ts-rs/specta)
│  └─ theme/               # Rust · paleta generada desde @gitraptor/tokens para TUI/CLI
├─ packages/
│  ├─ ui-kit/              # React · @gitraptor/ui (ADR-GRP-003)
│  ├─ design-tokens/       # @gitraptor/tokens · colores, tipografía, espaciado, temas
│  ├─ graph-renderer/      # @gitraptor/graph · grafo Git en Canvas/WebGL
│  └─ core-client/         # @gitraptor/client · cliente TS tipado del motor (tipos de crates/api)
└─ docs/
```

**Creación por fase:**
- **MVP (Fase 1):** `apps/cli`, `apps/mcp`, `crates/{core,policy,git,api,theme}` y `packages/design-tokens`.
- **Fase 3:** `apps/desktop`, `apps/vscode-extension`, `packages/{ui-kit,graph-renderer,core-client}`. No se crean antes para no cargar el monorepo con paquetes vacíos.

## Alternativas consideradas

- **Nx integrated (un solo `package.json`):** da más control centralizado, pero encaja peor con un repo políglota y con publicar paquetes independientes (VSIX, crates). Se descarta a favor de package-based, a pedido del equipo.
- **Turborepo:** liviano, pero sin soporte de primera clase para Rust ni grafo de proyectos políglota.
- **Solo Cargo workspace + pnpm sin orquestador:** sin caché ni `affected`, y la CI escala peor.
- **Moon / Bazel:** potentes para políglota, pero con curva y operación mayores para un equipo pequeño.

## Consecuencias

- ✅ Un solo repo, con caché y `affected` para Rust y TS, y el grafo de dependencias visible (`nx graph`).
- ✅ El UI kit y el cliente tipado se reutilizan entre la app de escritorio y la extensión.
- ⚠️ `@monodon/rust` es un plugin **comunitario**. **Mitigación:** fijar la versión. Si falla, el plan B son targets `nx:run-commands` sobre `cargo`.
- ⚠️ Cargo comparte el directorio `target/`. **Mitigación:** un `target-dir` distinto por target cuando haga falta, para evitar que los outputs cacheados se pisen.
- ⚠️ Nx Release para crates de Rust puede necesitar versionado legacy con `@monodon/rust`. **Mitigación:** validarlo al configurar los releases. La distribución principal son binarios y VSIX, no crates.io.
- ⚠️ No hay plugin oficial de Nx para Tauri. **Mitigación:** scripts de `package.json` (`tauri dev` / `tauri build`) como targets inferidos.

## Referencias

- [BRD-GRP-001 — Documento de negocio de GitRaptor](../../business/gitraptor-documento-de-negocio.md)
- [Nx — Add a Rust application to an Nx workspace](https://nx.dev/docs/kb/add-rust-to-nx-workspace)
- [Nx blog — Polyglot monorepos with Nx, TanStack and Rust](https://nx.dev/blog/polyglot-nx-monorepo-rust-tanstack)
- [@monodon/rust (npm)](https://www.npmjs.com/package/@monodon/rust)
- [Nx Release with Rust](https://nx.dev/docs/guides/nx-release/publish-rust-crates)
- [Tauri — Monorepo integration discussion #7368](https://github.com/orgs/tauri-apps/discussions/7368)
