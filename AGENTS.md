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
- **cargo-nextest (opcional, para certificar):** `cargo install cargo-nextest --locked`. `cargo nextest run --workspace --profile ci` corre la misma suite que `cargo test --workspace` y escribe el informe JUnit en `target/nextest/ci/junit.xml` (lo que detecta `layer-detect` de nassa-core para la línea base de `/implement`). La config está en `.config/nextest.toml`: `slow-timeout` con `terminate-after` (un test colgado muere a los 240 s). El CI sigue con `cargo test`. Los doctests no los corre nextest: `cargo test --doc`. Un target con `harness = false` debe hablar el protocolo libtest (`--list --format terse`, `--exact`), como `crates/git/tests/repo_intact_exec.rs`.
- **Adaptador de JUnit para `/implement` (`tools/test/nextest-junit.mjs`):** nextest nombra cada `<testsuite>` por el binario (`gitraptor-core::us_tmc_013`) y el plugin nassa-core solo reconoce un test del contrato por la ruta del archivo, así que la línea base B1 salía en rojo. `node tools/test/nextest-junit.mjs [args de nextest]` (por defecto `--workspace`) corre `cargo nextest run --profile ci`, devuelve su mismo código de salida y escribe `target/nextest/ci/junit-paths.xml` con un `<testsuite>` por archivo fuente (ruta relativa a la raíz; los archivos salen de `cargo metadata`). Los contratos de Rust de `/implement` lo declaran así (la fijación de la suite es `suite.command`; `layers.runtime.report` es el informe que lee el plugin):

  ```json
  "suite": { "command": "node tools/test/nextest-junit.mjs" },
  "layers": { "runtime": { "commands": [{ "id": "suite", "cmd": "node tools/test/nextest-junit.mjs" }], "report": "target/nextest/ci/junit-paths.xml" } }
  ```

  Pruebas del adaptador: `node --test tools/test/nextest-junit.test.mjs`. Un test unitario se asigna al archivo más profundo que exista para su módulo; si un binario no se resuelve, conserva el nombre original y el adaptador avisa por stderr. El arreglo nativo corresponde al plugin (soportar los nombres de binario de nextest con `cargo metadata`); cuando exista, este adaptador sobra.
- **No inventes resultados:** si algo falla o no se pudo verificar, dilo en el PR.

## Pull requests

- Título en Conventional Commits. Descripción con qué cambia, cómo se verificó y qué queda pendiente.
- Referencia los IDs correspondientes (BR-xx, US-xxx, ADR-xxx).
