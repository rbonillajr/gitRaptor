# Preparación de la corrida · US-DSK-004 (flota en vivo)

Sin diseño: el plan es [DS-US-DSK-004](../requirements/features/desktop/dev-specs/US-DSK-004-flota-en-vivo.md) (`spec-ready`). Preparada en el loop principal (2026-11-09), en modo rápido (sin línea base de suite completa; el resultado será SIN CERTIFICAR). Rama `feat/US-DSK-004-live-fleet` sobre `origin/main` con US-DSK-003 ya mergeada (#336).

## Alcance

- **Ya hecho antes de la corrida** (commits de la rama, en verde, por eso no son criterios): las primitivas `WorktreeCard` y `CardSkeleton` (`src/ui`), `fleet-model.ts`, `agent-colors.ts`, `useNow.ts`, `FleetView.tsx`, `RepoTree.tsx` y sus tests, y los tipos TS de los mensajes del repo (`engine-types.ts`).
- **Por hacer** (criterios `D4-*`): T001 y T002 (DTO, `ingest`, `HubCmd::Open`, `fleet_open`), T003 (reducers de caché, `selectedRepoId`, `useFleet`), el cableado de `FleetView` y `RepoTree` en `EngineCenter`, `AppShell` y `App`, y T006 (marcas de frescura). T007 es manual (macOS) y va en el PR.

## Comprobación de deriva (contra `origin/main`, con #336 dentro)

| Cita de la DS | Estado |
|---|---|
| `HubCmd::Open { repo_id }` | existe y el supervisor lo acepta e ignora (US-DSK-003); T002 lo implementa |
| `Supervisor`, `Sink`, `Ctx`, `seq`, `resync` | iguales a lo que cita la DS (DS-US-DSK-003 mergeada) |
| `ingest::{global, repo_summary, candidate, engine_state, engine, global_event}` | existen; faltan `key`, `repo`, `worktree`, `session`, `pair` |
| `EngineMessage` | sin las variantes del repo; T001 las suma |
| `EngineHub::open`, `commands/fleet_open.rs` | no existen; T002 |
| `engine-types.ts` | ya trae `repo`, `worktrees`, `sessions`, `repoSession`, `prediction`, `repoClosed` y `FleetPerf` |

## Decisión del orquestador (2026-11-09)

El mensaje de una sesión del repo abierto se llama `repoSession` (`{ type: "repoSession", repoId, session, perf }`), no `session`: ese nombre ya lo usa US-DSK-003 para la sesión del ámbito global. Registrada en la DS (T002, T003, T006, § Contratos compartidos).

## Ajustes de preparación

- **A1 · Esqueleto.** Antes de los tests, un experto deja compilando los tipos de `wire.rs` (DTO y variantes), las firmas de `ingest` (`key`, `repo`, `worktree`, `session`, `pair`) y `EngineHub::open`, con cuerpos mínimos que no hacen el trabajo. Sin `todo!()` en rutas de producción.
- **A2 · Tests propios.** `ingest::key` y la regla de `detached_at` van a `tests/fleet_ingest.rs`, no a un módulo `tests` de `src`: el tester solo escribe archivos de test y la regla R4 los congela.
- **A3 · Fixture.** `tests/fixtures/engine-messages.json` se amplía con las variantes del repo; la escribe el tester serializando los tipos reales de `wire.rs`. `engine_wire.rs` solo compara (decisión de US-DSK-003, 2026-11-09).
- **A4 · Un solo `cargo test` por comando.** Cada criterio de Rust construye `apps/desktop/dist` antes.
- **A5 · Primera compilación.** `target/` se reutiliza del PR anterior.
