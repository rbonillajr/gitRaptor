# Preparación de la corrida · US-DSK-008 (conflictos previstos en la app)

Sin diseño: el plan es [DS-US-DSK-008](../requirements/features/desktop/dev-specs/US-DSK-008-conflictos-previstos.md) (`spec-ready`, PR #342). Preparada en el loop principal (2026-11-09), en modo rápido (sin línea base de suite completa; el resultado será SIN CERTIFICAR). Rama `feat/US-DSK-008-predicted-conflicts` sobre `origin/main` (8acbf511).

## Alcance

- **Por hacer** (criterios `D8-*`): T001–T007 de la DS. T008 (verificación manual en macOS) va en la descripción del PR.
- **Esqueleto antes de los tests** (A1, como en US-DSK-004): tipos de `wire.rs`, firmas de `ingest`, `shell_command.rs`, `prediction.rs` y los stubs de `src/conflicts`, `ConflictBand`, `engine-types.ts` y `ui-store`, con cuerpos mínimos que compilan y no hacen el trabajo. Sin `todo!()` en rutas de producción.
- **Sin comandos IPC nuevos**: `commands.json` y `capabilities/main.json` están en `deny_paths`.
- **Fuera de alcance**: G1, G2, G4 y G11 de la DS (con dueño); la capa del grafo (US-DSK-005).

## Comprobación de deriva (contra `origin/main`)

| Cita de la DS | Estado |
|---|---|
| `PairSummary`, `MAX_LIST_PAIR_IDS`, `validate_list`, `page` (`crates/api/src/prediction.rs`) | iguales |
| `launcher::bundled_raptor`, `daemon_path` del informe `ready` (`link.rs`, `launcher.rs`) | existen; `Link::daemon_exe` y la ruta guardada son de T002 |
| `wire::stop_command` | existe (`wire.rs`) |
| `PairDto` | la forma corta de US-DSK-004; T001 la amplía |
| `Esc` en `useShortcuts`, `Dialog`, `Popover`, `Toast` | sin `defaultPrevented`; D18 lo añade |
| `tests/boundaries.rs` › `no_os_notifications` | no existe; lo escribe el tester |

## Ajustes de preparación

- **A1 · Esqueleto** antes del criterion-tester (rust-expert y web-expert, sin tocar tests).
- **A2 · Tests propios.** Los tests de Rust van en `tests/prediction_supervisor.rs` y `tests/shell_command.rs` (archivos de test, R4); el unitario de `PredictionTracker` queda cubierto por el supervisor porque `mod prediction` es privado.
- **A3 · Tests existentes que cambian de forma** (fixture congelada de `engine_wire`, `engine-cache.test.ts`, `fleet_ingest`): los actualiza el tester, no el experto.
- **A4 · Un `pnpm build` antes de cada `cargo test`** del crate de escritorio (necesita `apps/desktop/dist`).
- **A5 · Compilación.** `target/` clonado de la semilla `seed-main`; sin `cargo clean`; `CARGO_BUILD_JOBS=4`.
- **Build en dos tramos**: backend (rust-expert, `src-tauri`) y frontend (web-expert, `src`). Los dos son `sonnet`.
