# Preparación de la corrida · US-TMC-012 (solape en dos fases)

Plan: [DS-US-TMC-012](../requirements/features/time-machine/dev-specs/US-TMC-012-solape-otro-actor.md) (sin diseño nuevo). Contrato: [us-tmc-012-overlap.contract.json](us-tmc-012-overlap.contract.json). Preparó el loop principal (opus) el 2026-10-28.

## Comprobación de deriva (citas de la Dev Spec frente a `main` 3a6a6060)

Todas las citas siguen ciertas. Matices, sin bloqueo:

- "`Observation` con `cause_operation`" es una instantánea de nivel `SnapshotLevel::Observation` cuyo `SnapshotRecord` lleva `cause_operation`, `cause_event_seq` y `engine_mark` (`oplog/model.rs`). No existe un tipo `Observation`.
- La CLI pide en `connection.accept` todas las capacidades que el daemon sirve (`apps/cli/src/client/engine.rs:62`): la regla de T006 "la CLI pide `CAP_TM_OVERLAP`" se cumple al añadirla a `capabilities` de `crates/api/src/methods/timemachine.rs`.
- `catalogs_match` vive en `apps/cli/src/i18n.rs`, no en `undo/mod.rs`. `commands/restore.rs` tiene su propio `reason_key`: una variante nueva de `TmRejectReason` también se mapea allí.
- `raptor-fake-agent` no es un binario del workspace: el e2e copia un ejecutable con ese nombre (patrón de `apps/cli/tests/raw_git_undo.rs`).
- La suposición de la nota técnica de T005 se cumple: `UndoStep::self_annotated()` es `true` y `ProtectedOperation::run` no reescribe una operación que el paso dejó en `rejected`; devuelve `ProtectedError::Step`.

## Supuestos

- Los tests del contrato van en archivos propios (regla R4 del guard): `crates/api/src/timemachine_overlap_tests.rs`, `crates/core/src/timemachine/store/changed_between_tests.rs`, `crates/core/src/timemachine/overlap_tests.rs`, `crates/core/tests/tm_apply_keep.rs` y `apps/cli/src/undo/overlap_tests.rs`. La Dev Spec pone algunos en `store/mod.rs` y `tm_apply.rs`; cambia solo la ubicación, no lo que prueban. `tm_apply.rs` no entra en el contrato para que el experto pueda actualizar sus literales de `PlanWorktree` al añadir `keep`.
- Antes de medir la línea base, el loop principal añade los esqueletos que hacen compilar los tests en rojo (tipos y firmas de T001–T004 con cuerpos que fallan, nunca `todo!()` en un camino de producción) y los declara con `#[cfg(test)] #[path = "…"] mod …;`.
- Salto de construcción en **opus** (NFR-01: el solape decide qué se escribe), crítico y revisor en opus, tester y conformidad en sonnet.
- G1 (solape de la restauración, US-TMC-024) queda fuera. `crates/core/src/timemachine/restore.rs` construye `PlanWorktree` con literales (líneas 490, 508 y 549), así que **solo** se le añade `keep: BTreeSet::new()` (o el valor vacío del tipo); ningún otro cambio en la restauración (crítico, PC-1).
- `crates/api/src/timemachine_overlap_tests.rs` se monta en la raíz del crate, como `timemachine_inferred_tests` (`crates/api/src/lib.rs:32-34`): `#[cfg(test)] #[path = "timemachine_overlap_tests.rs"] mod timemachine_overlap_tests;` (crítico, PC-2).
- `diff_trees` se prueba en su crate, `crates/git/src/tm_write/store/diff_trees_tests.rs`, montado en `crates/git/src/tm_write/store/mod.rs` como `#[cfg(test)] #[path = "diff_trees_tests.rs"] mod diff_trees_tests;`: un objeto ausente dentro de un subárbol igual en los dos lados demuestra que no se recorre (crítico, PC-3).
- Archivos que también se tocan aunque la § Estructura de ficheros no los nombre (crítico, PC-4): `apps/cli/src/commands/restore.rs` (su `reason_key` exhaustivo necesita las dos variantes y sus claves `restore.reason.overlap*` en/es) y `crates/core/tests/tm_kept_temps.rs` (un literal de `PlanWorktree` que necesita `keep`).
- **Ajustes tras el tester** (decisiones de la preparación; no cambian el diseño):
  - `conflicts_are_capped_with_the_real_total` pasa a integración (`us_tmc_012.rs`, por el daemon): el tope de 50 y el `total` viven en el cable (`TmOverlap`), no en `OverlapReport`. El experto lo aplica al convertir el informe a `TmOverlap` en `conn.rs`.
  - `mcp_gets_the_reason_without_paths` pasa a la forma de cable (`timemachine_overlap_tests.rs`): el undo por MCP sigue en `SCOPE_REFUSED` hasta DEP-MCP-4, así que el canal no se puede ejercitar extremo a extremo. La rama MCP de `conn.rs` (que nunca envía `overlap` ni rutas, ni al log) la verifica la revisión de seguridad.
  - `the_same_agent_session_is_not_another_actor` pasa al e2e (`undo_overlap_process.rs`): el arnés en proceso no inyecta eventos crudos; el agente simulado los produce con su sesión.
  - API de `apps/cli/src/undo/overlap.rs` que fijan los tests: `pub(super) fn rejection_lines(tr: Tr, reason: TmRejectReason, overlap: Option<&TmOverlap>) -> Vec<String>` y `pub(super) fn kept_lines(tr: Tr, kept: &[KeptPath]) -> Vec<String>`; claves nuevas `undo.reason.overlap`, `undo.reason.overlap-unverified` y el grupo `undo.overlap.*` al final del catálogo.
- **Ajustes tras la fase A (corrida abortada en P0 antes de congelar, 2026-10-28):**
  - Los tests de cable esperaban `Untrusted` como texto plano; el cable es `{"untrusted": …}` (SEC-12, `crates/api/src/untrusted.rs`). Corregidos los tests, no el cable.
  - T001 es solo datos y el esqueleto lo dejó casi completo. Para que la base mida en rojo, el esqueleto deja sin hacer `UndoResult::for_mcp` → `kept` (devuelve 0); la fase B lo cuenta. Eso deja en rojo TMC012-WIRE y TMC012-SEC-MCP.
  - `paths_with_control_characters_are_printed_sanitized` exige ahora que la línea exista y salga saneada (pasaba con una lista vacía).
  - Se retira el criterio TMC012-SAME-ACTOR. Su test (`an_unattributed_undo_ignores_unattributed_edits`) prueba algo que ya existe y queda en `us_tmc_012.rs` como guarda de regresión, fuera de los criterios. Su variante con un agente no se puede construir con el undo de la última operación sin inventar arnés. D4 sigue cubierto por S1 (sin atribuir frente a claude-1) y por TMC012-CLASSIFY (tramo mixto).
  - Se retira también TMC012-SAME-SESSION: afirma "sin solape", así que pasa con los esqueletos por construcción. `the_same_agent_session_is_not_another_actor` queda en el e2e como guarda de regresión (corregido su montaje: la rama existe antes de que el agente la mueva). La § 9.4 de la Dev Spec lo sitúa en `us_tmc_012.rs`: vive en `apps/cli/tests/undo_overlap_process.rs` y lo ejecuta el verify de TMC012-E2E (archivo entero). El contrato queda en 20 criterios.
  - `UndoError::Overlap` lleva `Box<OverlapReport>` (clippy `result_large_err`); desvío menor de la firma de la spec.
- Lo que la Dev Spec no cubre y aquí se sigue: tests de integración en `crates/core/tests/` con `cfg(any(target_os = "macos", target_os = "linux"))` y el e2e solo en macOS, como `us_tmc_013.rs` y `raw_git_undo.rs`.
