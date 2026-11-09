# Preparación de la corrida · US-DSK-007 (bloqueo explicado en la app)

Sin diseño: el plan es [DS-US-DSK-007](../requirements/features/desktop/dev-specs/US-DSK-007-bloqueo-explicado.md) (`spec-ready`, PR #342). Preparada en el loop principal (2026-11-09), en modo rápido (sin línea base de suite completa; el resultado será SIN CERTIFICAR). Rama `feat/US-DSK-007-explained-block` sobre `origin/main` (US-DSK-008 ya dentro).

## Alcance

- **Por hacer** (criterios `D7-*`): T001–T006 de la DS. T007 (verificación manual en macOS) va en la descripción del PR.
- **G7 resuelto antes del plan**: Arquitecto enmendó ADR-GRP-017 (§ 5: ruta de reserva del `raptor` con L-02; riesgo R-5b del portapapeles; `guard_blocks` solo lectura). **Decisión del orquestador (2026-11-09), validada por Arquitecto.** Criterio estático `D7-ADR`. Toca ADR-GRP-017: pasa `/security-review`.
- **Un comando IPC nuevo** (`guard_blocks`): `commands.json` y `capabilities/main.json` **no** están en `deny_paths`; el test de lista exacta (`commands.rs`) pasa a cinco comandos, no se relaja a «contiene».
- **Esqueleto antes de los tests** (como en US-DSK-004 y 008): tipos de `wire.rs`, firmas de `exception_command`/`ingest`, `HubCmd::GuardBlocks` ignorado, y los stubs de `src/guardrails`, con cuerpos mínimos que compilan y no hacen el trabajo. Sin `todo!()` en rutas de producción. El esqueleto **no** registra el comando en `commands.json` ni en la capability (eso es producción, tras la aprobación).
- **Fuera de alcance**: G1 y G3 (PO, motor), G12 (texto de la CLI sobre `guard cancel`, PO de Guardrails), G2, G4; la capa del grafo (US-DSK-005).

## Comprobación de deriva (contra la rama, = `origin/main`)

| Cita de la DS | Estado |
|---|---|
| `shell_command::{posix, raptor_path, confirm}` (D21 de 008) | existen |
| `CopyCommand.onCopied` (D22 de 008) | existe |
| `ui-store` `CenterView` con `main` y `pair`; `backToMain` | existen; T003 añade `guardrails` |
| `HubCmd` sin `GuardBlocks`; `commands.json` con 4 comandos | como dice la DS |
| `Toast` sin `srLabel` | como dice la DS (T005) |

## Ajustes de preparación

- **A1 · Esqueleto** antes del criterion-tester (rust-expert y web-expert, sin tocar tests).
- **A2 · Tests de Rust** en `tests/guard_command.rs` y `tests/guard_blocks.rs` (nuevos); `commands.rs` y `engine_wire.rs` (existentes) los actualiza el tester (lista exacta a cinco, fixture ampliada).
- **A3 · Un `pnpm build` antes de cada `cargo test`** del crate de escritorio (necesita `apps/desktop/dist`).
- **A4 · Compilación.** `target/` de la semilla; sin `cargo clean`; `CARGO_BUILD_JOBS=4`.
- **Build en dos tramos**: backend (rust-expert, `src-tauri`) y frontend (web-expert, `src`). Los dos `sonnet`.
