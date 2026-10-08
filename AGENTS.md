# AGENTS.md — GitRaptor

Instrucciones para cualquier agente de IA (Claude Code, Cursor, Codex, Copilot) que trabaje en este repositorio.

## Qué es GitRaptor

El copiloto de Git para equipos que programan con agentes de IA. Tiene tres pilares:
- **Cockpit:** agentes y worktrees en vivo, con predicción de conflictos.
- **Time Machine:** snapshots y undo universal.
- **Guardrails:** políticas por repo.

Se entrega como **CLI/TUI (`raptor`) + servidor MCP**. Estado actual: **pre-MVP** (definición + spike).

## Fuente de verdad: lee esto antes de trabajar

| Documento | Para qué |
|---|---|
| [`docs/business/gitraptor-documento-de-negocio.md`](docs/business/gitraptor-documento-de-negocio.md) | Alcance (BR-xx), NFRs, decisiones (D1–D3) y preguntas abiertas |
| [`docs/architecture/decisions/`](docs/architecture/decisions/) | ADRs: stack (001), monorepo Nx (002), design system (003), estado del frontend y UX (004) |
| [`docs/design-system/README.md`](docs/design-system/README.md) | Design system v0 (TUI, CLI, tokens, contenido) |
| [`docs/requirements/release-plan.md`](docs/requirements/release-plan.md) | Plan de releases y control de estado: hitos (M1, M2, M3, v0.1.0), criterio de salida, avance y fecha estimada por velocidad real |
| [`docs/architecture/extender-sin-archivos-compartidos.md`](docs/architecture/extender-sin-archivos-compartidos.md) | Cómo añadir un método, un error, un mensaje, un subcomando o un módulo del daemon tocando solo archivos propios (ADR-GRP-016) |

Si una tarea contradice estos documentos, **detente y pregunta**; no improvises una decisión de arquitectura.

## Alcance: qué se construye ahora

- **MVP (Fase 1):** `apps/cli`, `apps/mcp`, `crates/{core,policy,git,api,theme}` y `packages/design-tokens`.
- **Agentes soportados en el MVP:** solo Claude Code. Después, uno por uno: Codex y luego Cursor (D2, revisada el 2026-10-03).
- **No construir todavía (Fase 3):** app de escritorio (Tauri + React), extensión de VS Code/Cursor, UI kit React, Storybook ni Motion. Esas definiciones están en los ADRs **solo como referencia**.

## Stack (ADR-GRP-001 / 002)

- **Rust** para el motor, la CLI/TUI y el MCP: gitoxide para leer, Git CLI del sistema para escribir, `ratatui`, `clap` y `rmcp`.
- **Monorepo Nx package-based:** pnpm workspaces + Cargo workspace, con `@monodon/rust`.
- **TypeScript** solo donde se indique: tokens con Style Dictionary y, en la Fase 3, la UI.

## Flujo de trabajo con Git

- **Trunk-based:** una rama corta por historia o tarea, **un worktree por rama, un agente por worktree**.
- **Prefijos de rama:** `spike/`, `docs/`, `feat/<story-id>-<slug>`, `fix/`, `chore/`.
- **Worktrees:** se crean y cierran con Orca, que los ubica en `~/orca/workspaces/gitRaptor/`. Al crearlos corre el setup de `orca.yaml` (`pnpm install` + `cargo fetch`); al cerrarlos (`orca worktree rm --worktree branch:<rama> --run-hooks`) bloquea si hay commits sin pushear. La rama no se borra.
- **Nombre de rama en worktrees de Orca:** Orca crea la rama como `rbonillajr/<nombre>`. Antes del primer commit, renómbrala a la convención (`git branch -m docs/<slug>`, `feat/<story-id>-<slug>`, etc.).
- **Nunca hagas commit ni push directo a `main`.** Todo entra por PR y se mergea con **rebase and merge**. Después del merge se borra la rama remota y se cierra el worktree.
- **Primer push:** `git push -u origin <rama>`. Las ramas se crean sin upstream.
- **Commits:** Conventional Commits en inglés (`feat:`, `fix:`, `docs:`, `chore:`, `test:`, `refactor:`), pequeños y atómicos.
- **Mantente dentro del propósito de tu rama.** Si encuentras trabajo fuera de alcance, anótalo en el PR; no lo hagas.

## Reglas de calidad

- **Idioma:** documentación y artefactos en **español**. Código, identificadores y mensajes de commit en **inglés**. Los mensajes al usuario final van con i18n (en/es).
- **Cero pérdida de datos (NFR-01):** toda operación que modifique un repo debe ser recuperable. Los tests usan repos temporales, nunca este repo.
- **Seguridad del MCP (NFR-02):** sin shell (argv fijo), validación de entradas, allowlist de repos.
- **Tests obligatorios** para todo cambio de comportamiento. `cargo clippy` y `cargo test` (o `nx affected -t lint test`) deben pasar antes del PR.
- **No inventes resultados:** si algo falla o no se pudo verificar, dilo en el PR.

## Pull requests

- Título en Conventional Commits. Descripción con qué cambia, cómo se verificó y qué queda pendiente.
- Referencia los IDs correspondientes (BR-xx, US-xxx, ADR-xxx).
