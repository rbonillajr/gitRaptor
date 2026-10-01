# GitRaptor

> El copiloto de Git para equipos que programan con agentes de IA.

GitRaptor muestra en vivo qué están haciendo tus agentes en el repo, impide que rompan algo y te deja deshacer cualquier cosa. Funciona con cualquier agente (Claude Code, Cursor, Codex, Copilot), en Windows, macOS y Linux, desde la terminal (CLI/TUI) o desde el propio agente vía MCP.

**Estado:** definición de producto y arquitectura (pre-MVP).

## Documentación

| Documento | Descripción |
|---|---|
| [Documento de negocio](docs/business/gitraptor-documento-de-negocio.md) | BRD-GRP-001: problema, research de mercado, alcance por fases, NFRs, riesgos |
| [ADR-GRP-001 — Stack tecnológico](docs/architecture/decisions/ADR-GRP-001-stack-tecnologico.md) | Rust (motor, CLI/TUI, MCP) + Tauri/React (escritorio) + TypeScript (extensión) |
| [ADR-GRP-002 — Monorepo Nx](docs/architecture/decisions/ADR-GRP-002-monorepo-nx.md) | Nx package-based con pnpm y Cargo workspaces |
| [ADR-GRP-003 — Design system](docs/architecture/decisions/ADR-GRP-003-design-system.md) | Tokens, UI kit, reglas de componentes y movimiento |
| [ADR-GRP-004 — Estado del frontend y UX](docs/architecture/decisions/ADR-GRP-004-estado-frontend-ux.md) | Capas de estado y patrones de UX (Fase 3) |
| [Design system](docs/design-system/README.md) | Alcance v0 (MVP: CLI/TUI + MCP) |

## Desarrollo

Requisitos: Node 22+, pnpm 10 y rustup (la versión de Rust la fija `rust-toolchain.toml`).

```sh
pnpm install
pnpm nx run-many -t lint test build   # todo el workspace
pnpm nx affected -t lint test build   # solo lo que cambió respecto a main
pnpm nx run gitraptor-cli:run         # ejecuta `raptor`
pnpm nx graph                         # grafo de proyectos
```

| Proyecto | Ruta | Qué es |
|---|---|---|
| `gitraptor-cli` | `apps/cli` | Binario `raptor` (CLI + TUI) |
| `gitraptor-mcp` | `apps/mcp` | Binario `raptor-mcp` (servidor MCP) |
| `gitraptor-core` | `crates/core` | Motor: watcher, oplog/snapshots, conflictos |
| `gitraptor-policy` | `crates/policy` | Guardrails (`policy.yaml`) |
| `gitraptor-git` | `crates/git` | Capa Git (gitoxide + Git CLI) |
| `gitraptor-api` | `crates/api` | Contrato JSON-RPC/eventos |
| `gitraptor-theme` | `crates/theme` | Paleta para TUI/CLI |
| `@gitraptor/tokens` | `packages/design-tokens` | Design tokens (DTCG + Style Dictionary) |
