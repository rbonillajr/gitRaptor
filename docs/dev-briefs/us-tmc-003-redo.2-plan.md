# Brief — `raptor redo` (US-TMC-003), preparación

Plan aprobado: [DS-US-TMC-003](../requirements/features/time-machine/dev-specs/US-TMC-003-redo.md) (spec-ready, `gaps_blocking: 0`; enmienda E1 a ADR-TMC-003 ya aplicada). Este Brief no rediseña. Contrasta las citas de la spec con `origin/main` (`fa3e8d28`, 2026-11-09), fija solo lo que la spec deja abierto y añade los pendientes de redo que dejó US-TMC-011 (PR #325). Contrato: [`us-tmc-003-redo.contract.json`](./us-tmc-003-redo.contract.json). Preparado en el loop principal (opus).

## Deriva

"Vigente" = el código dice lo que la spec da por hecho. Desde que se escribió la spec entraron US-TMC-012 (#316), US-TMC-010 (#319) y US-TMC-011 (#325). US-TMC-014 (`on_remote`) **no** está en `main`.

- `must_read`: los 17 ficheros existen. Vigente.
- § Contexto: `timemachine.redo` está en el catálogo con `mcp: false` e `implemented_by: US-TMC-003` (`crates/api/src/methods/timemachine.rs:59`). Responde `NOT_IMPLEMENTED` desde `tm_command` (`crates/core/src/channel/conn.rs:4620`, brazo `WriteRoute::TimeMachine(story)` en `:892`). Vigente.
- T001: `RedoParams { worktree, operation_id, surface }` sin `confirmation` (`crates/api/src/timemachine.rs:631`). `TmRejectReason` termina en `NotSelectable, SelectiveUndoBelow, AttributionUnavailable` (`:248-256`, de 011): `NothingToRedo` y `RedoInvalidated` van detrás. **Deriva D-A**: `UndoResult` ganó `kept_total` (`:480`) y `undone_operation_ids` (`:483`). `RedoResult` copia `kept` **y** `kept_total` (misma regla `CAP_TM_OVERLAP`). `undone_operation_ids` no aplica: el redo rehace un undo, cuyo id ya va en `redone_undo_id`.
- T002: `UndoStack` (`crates/core/src/timemachine/oplog/stack.rs:58-66`) tiene ahora `number` y `barriers` (de 011). `build` vacía `redo` en `Do` (`:79`). El `Redo` quita la barrera del undo selectivo y reordena `done` (`:96-106`). `next_redo` (`:133`). Vigente: `invalidated_by` se añade sin tocar barreras ni orden. **Pendiente de 011 cubierto aquí**: el redo de un undo selectivo devuelve **los mismos ids** de `Target::Undo(set)` y levanta la barrera. La pila ya lo hace (`stack_barrier_tests.rs`). Lo que falta es el redo en sí; lo prueban los tests de 011 de § Pendientes de US-TMC-011.
- T003: `PlanKind { Undo, Restore }` y `tag()` (`crates/core/src/timemachine/confirm/mod.rs:29-41`). Vigente. **Deriva D-B**: `Confirmation::gate_over` recibe ahora `taken_back: &[&str]` (de 010, `undo.rs:737-749`). El redo pasa `&[]`: el undo rehecho ya entra en el hash por `PlanFacts.undone_id`, y `PlanKind::Redo` separa el hash del de un undo.
- T004:
  - `undo_last` ya no existe: es `undo_selected(repo, who, channel, confirm, env, selection)` (`undo.rs:1200`). Lock, `settle`, `settle_all`, `RawSide` y `raw_of_all` están en `:1215-1290`. El redo sigue ese mismo orden, sin `Selection`.
  - `plan` (`:541`) contiene el bloque que la spec manda extraer a `materialize`: worktrees registrados, clave y rama por `meta`, `head_branch` (`:756-786`). Vigente.
  - **Deriva D-C**: el subtipo del paso no es un campo, es `ProtectedStep::subtype()` calculado (`SELECTIVE_UNDO_SUBTYPE` si `plan.selective`, si no `"undo"`, `:1155-1162`). Se cumple la spec convirtiéndolo en el campo `subtype: &'static str` de `UndoStep`. `undo_selected` pone el valor de hoy y el redo pone `"redo"`. El registro del undo no cambia.
  - **Deriva D-D**: `record_rejection` (`:899`) fija `OperationKind::Undo` y `Target::Undo`. El redo registra sus rechazos con su propia función en `redo.rs` (`kind: Redo`, `Target::None` o `Target::Redo(U)`, D10), sobre el mismo `NewOperation` + `advance_operation(Rejected)`. `undo.rs` no cambia aquí.
  - **Deriva D-E**: el solape se llama como en el undo, `overlap::check_with(store, &OverlapInput{..}, events_complete)` + `check_tail_with` (`overlap.rs:114`, `:166`). Con `undone = [OpRef::Oplog(U)]` la base es el ancla de U (`base_point_of`, `:310`). Vigente.
  - `restore::restore_permission(who, channel, owners)` (`restore.rs:253`). Vigente.
  - `undo_stack_in` cuenta un `redo` terminado solo si su undo existe, uno interrumpido como `Do`, y no cuenta uno rechazado (`stack.rs:177-205`). Vigente (nota técnica de la spec).
- T005: `tm_undo` (`conn.rs:4221`), `undo_error` (`:3470`) y `undo_result` (`:3610`, con `with_kept`) existen. `tm_command` exige `require_attributed` solo a `TM_UNDO | TM_RESTORE` (`:4643`). El redo sin id pasa a su propio brazo. Por MCP sigue `METHOD_NOT_FOUND` (`channel_protected.rs:783`). Vigente.
- T006: `commands!` con `Undo(undo)` (`apps/cli/src/commands/mod.rs:44`), `worktree_root` (`undo/mod.rs:48`), `notices::show_pending` (`undo/notices.rs:17`), `confirm::call` y `confirm::required_key` (`undo/confirm/mod.rs:40`, `:115`), `undo/overlap.rs`. Vigentes. **Deriva menor**: hay dos `match` exhaustivos sobre `TmRejectReason` que ganan los dos motivos nuevos: `apps/cli/src/undo/mod.rs:402` (clave del undo) y `apps/cli/src/commands/restore.rs:247`. Se reparten como `TargetUnavailable` (un undo y un restore nunca los reciben).
- **Deriva D-F — `on_remote` (US-TMC-014)**: no está en `main`; la enmienda 2026-10-28 de la spec no aplica a este PR.
- § 9.3: el cliente en proceso de `us_tmc_013_common` es "sin atribuir". **Deriva D-G**: desde 011 existe un arnés de integración con agentes simulados, `crates/core/tests/us_tmc_011_common/mod.rs` (`spawn_agent`, `agent_commit`, `as_agent`, `redo_call` en `:798`), independiente de la sesión de Claude Code. Los escenarios 5 y 6 se prueban ahí, con daemon y Git reales, en vez de en el e2e (§ Decisiones de preparación, P1).

No hay BLOQUEO: ninguna deriva cambia el diseño.

## Pendientes de US-TMC-011 (parte de esta historia)

Los tres `// redo: pending US-TMC-003` de `crates/core/tests/us_tmc_011.rs` (`:585`, `:628`, `:705`) y el de la demo `apps/cli/tests/m2_demo_undo_agent.rs:453`:

| Test | Qué gana |
|---|---|
| `redo_restores_the_set_and_lifts_the_barrier` | `raptor redo` (desarrollador, con confirmación) rehace el undo por agente. Las rutas vuelven al estado de después del trabajo del agente. El registro del redo es `Target::Redo(U)`, y U conserva su `Target::Undo(set)` sin cambios. La barrera desaparece: el siguiente undo simple ya no es `selective-undo-below` |
| `plain_undo_after_redo_takes_the_latest_operation` | Tras el redo, un undo simple deshace la operación más reciente (`two.txt` de claude-1), no una de debajo |
| `a_correction_after_the_undo_does_not_change_what_it_undid` | Tras la corrección, el redo devuelve exactamente lo que deshizo U (los mismos ids y archivos), aunque la atribución vigente haya cambiado |
| demo M2 (`m2_demo_undo_agent.rs`) | Paso final: el desarrollador corre `raptor redo` con la confirmación en la pty. El worktree objetivo vuelve al estado de después del trabajo de su agente y los demás worktrees no cambian |

Los comentarios `redo: pending` se quitan y el doc de la demo (`:4`) deja de decir "pending".

## Decisiones de preparación

Ninguna cambia el diseño ni el producto. Por la regla de validación proporcional no se consulta al Arquitecto ni al PO.

- **P1 — dónde se prueban los escenarios 5 y 6.** Van en `crates/core/tests/us_tmc_003_agents.rs`, con el arnés de 011. La spec los ponía en el e2e porque no había agentes en integración (§ 9.3). El e2e `redo_process.rs` prueba la CLI: textos en/es, `--json`, rechazo con código 1. Mismos nombres de test que la spec.
- **P2 — tests que ya pasan hoy, fuera de los criterios.** `redo_is_not_offered_over_mcp` (ya lo cubre `channel_protected.rs::mcp_scope_comes_from_the_caller`) y `an_interrupted_redo_is_undone_back_to_its_prior` (la pila ya cuenta un redo interrumpido como `Do`, `stack.rs:181`) pasarían en verde antes del código, así que no son criterios. El primero queda como regresión. El segundo pasa a un unitario de la pila en `stack_redo_tests.rs`, que sí exige el campo nuevo (`redo_invalidated` es `None` tras un redo interrumpido). `a_redo_by_id_is_not_implemented` se fusiona con "sin id se sirve" (`redo_without_id_is_served_and_by_id_is_not`), que hoy está en rojo.
- **P3 — la demo M2 es `should`.** La demo no es fiable en macOS con la máquina cargada por TD-GRP-005 (atribución S3 fuera de ventana; ver ficha de US-TMC-011). El criterio que la cubre es `should`. El redo de un undo por agente ya lo exigen los criterios `must` de 011.

## Gaps y supuestos

- Supuesto: el destino del redo de un undo selectivo es el previo de ese undo, aplicado entero (no selectivo). Las rutas que otro actor cambió después se conservan con el `keep` del solape, y las de un mismo actor las cubre el previo del redo. Así lo fija DS-US-TMC-011 § "US-TMC-003 (redo)": "el redo de un undo selectivo vuelve a su previo".
- Supuesto: en el test de la corrección (D1 de 011), el redo lo pide el desarrollador. Los dueños se leen con la atribución vigente, así que puede pedir confirmación; el test la da con `TestConfirmation::Eligible`.
- US-TMC-011 sigue `partially-implemented` tras este PR por TD-GRP-005 y por XP-69. Solo sale de su lista de pendientes el redo.

### Convenciones observadas

- Unitarios en su propio fichero, montados con `#[cfg(test)] #[path = "<f>.rs"] mod <m>;` (`crates/api/src/lib.rs:34-44`, `oplog/stack.rs` → `stack_barrier_tests.rs`). Las líneas de montaje van en el commit de stubs; no las escribe el tester.
- Integración con daemon y Git reales: `us_tmc_013_common::{start, Running, params, rejected, confirm_data, token_of}`, huella `fx.fingerprint()`, `TestConfirmation::Eligible` (patrón de `us_tmc_002.rs`, `us_tmc_010.rs`). Con agentes simulados: `us_tmc_011_common::world()`.
- e2e: `apps/cli/tests/us_tmc_011_process.rs` y `undo_overlap_process.rs` (`#![cfg(target_os = "macos")]`, `env_clear()` + entorno mínimo, perfil temporal).
- i18n: `apps/cli/i18n/{en,es}/timemachine.txt`, grupo `redo.` nuevo al final (G2 de la spec).

### Arnés de actor (independiente de la sesión de Claude Code que corre las pruebas)

El de 011, sin cambios. El daemon de prueba arranca con `AgentMatcher::only(["raptor-fake-agent"])`. "claude-1" y "claude-2" son dos procesos `raptor-fake-agent`, y la sesión real que lanza `cargo test` nunca es agente. Todo test con un dueño comprueba antes su actor (`assert_is_claude_code`). El e2e usa `GITRAPTOR_AGENT_EXECUTABLES=raptor-fake-agent` (solo debug) con `env_clear()`.

### Inventario de reutilización

Solo lo que la spec deja abierto:

| Necesidad | Se reutiliza | Nuevo |
|---|---|---|
| Registro del rechazo de un redo | `NewOperation` + `advance_operation(Rejected)` como `record_rejection` | función propia en `redo.rs` (D-D) |
| Solape con eventos incompletos | `overlap::check_with` / `check_tail_with`, `raw_of_all`, `settle_all` | — |
| Subtipo del paso | `UndoStep` | campo `subtype` (D-C) |
| Resultado con `kept` cortado | `kept::` y la lógica de `undo_result(.., with_kept)` | `redo_result` |

## Topología

Tramos disjuntos. La spec ya los dibuja (§ Estructura de ficheros):

| Tramo | Tareas | Ficheros (producción) | Criterios |
|---|---|---|---|
| 1 · contrato | T001 | `crates/api/src/timemachine.rs`, `crates/api/src/methods/timemachine.rs` (comentario) | TMC003-WIRE |
| 2 · pila y confirmación | T002, T003 | `oplog/stack.rs`, `confirm/mod.rs` | TMC003-STACK, TMC003-CONFIRM |
| 3 · redo | T004 | `timemachine/redo.rs` (nuevo), `timemachine/mod.rs`, `timemachine/undo.rs` | TMC003-OWNERS (⛔4.1), TMC003-S1…S4, WALK, GUARD-4.2 (⛔4.2) |
| 4 · canal | T005 | `channel/conn.rs` | TMC003-SERVED, S5, S6, CONFIRM-AGENT, TOKEN, SELECTIVE |
| 5 · CLI | T006 | `apps/cli/src/commands/{redo,mod}.rs`, `apps/cli/src/undo/{redo,mod}.rs`, `apps/cli/src/commands/restore.rs` (un brazo), `apps/cli/i18n/{en,es}/timemachine.txt` | TMC003-CLI-TEXT, TMC003-E2E, TMC003-M2-DEMO |
| 6 · docs | T008 | `docs/architecture/design/api-contract-ipc.md`, `docs/architecture/xplat-pendientes.md` (XP-70), fichas y Dev Specs de US-TMC-003 y US-TMC-011, `backlog.md`, `release-status.md` | revisión |

Primer tramo trazador: 1 → 2 → 3 con `TMC003-S1` (el escenario 1 de punta a punta en integración, sin CLI).

Tests del contrato (los escribe el criterion-tester, en rojo): `crates/api/src/timemachine_redo_tests.rs`, `crates/core/src/timemachine/oplog/stack_redo_tests.rs`, `crates/core/src/timemachine/confirm/redo_tests.rs`, `crates/core/src/timemachine/redo_tests.rs`, `crates/core/tests/us_tmc_003.rs`, `crates/core/tests/us_tmc_003_agents.rs`, `crates/core/tests/us_tmc_011.rs` (solo los tres tests de § Pendientes), `apps/cli/src/undo/redo_tests.rs`, `apps/cli/tests/redo_process.rs`, `apps/cli/tests/m2_demo_undo_agent.rs` (solo el paso de redo).

Stubs inertes (commit propio antes de los tests, compilan, sin `todo!()` en rutas de producción): las dos variantes y `RedoResult`/`RedoParams.confirmation` en la API; `redo_invalidated()` que devuelve `None`; `PlanKind::Redo`; `redo.rs` con `redo_last` que devuelve `Internal("not implemented")`, `redo_owners` vacío y su `mod tests`; `apps/cli/src/undo/redo.rs` con su `mod tests` y los brazos de los dos `match` de la CLI.

## Modo de la corrida

Modo rápido (Rene, 2026-11-08): sin línea base de suite completa. Tras la firma se prueba de forma dirigida que los tests del contrato están en rojo por la razón correcta, se aborta P0 y se construye **SIN CERTIFICAR**. Antes de subir: tests del contrato, `cargo test -p gitraptor-core` **completo** (excepción NFR-01), `cargo test -p gitraptor-api`, `cargo test -p gitraptor-cli`, `cargo clippy --workspace --exclude gitraptor-desktop --all-targets -- -D warnings` y `cargo fmt --all -- --check`. La suite completa la corre el CI.
