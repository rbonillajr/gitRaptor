# Brief — `raptor undo --agent` (US-TMC-011), preparación

Plan aprobado: [DS-US-TMC-011](../requirements/features/time-machine/dev-specs/US-TMC-011-undo-por-agente.md) (spec-ready, `gaps_blocking: 0`, enmiendas E1–E4 a ADR-TMC-002/003/005 ya aplicadas). Este Brief no rediseña: contrasta las citas de la spec con el código de `origin/main` (`5d325072`, 2026-11-08) y fija solo lo que la spec deja abierto. Contrato: [`us-tmc-011-undo-by-agent.contract.json`](./us-tmc-011-undo-by-agent.contract.json). Preparado en el loop principal (opus).

## Deriva

"Vigente" = el código dice lo que la spec da por hecho.

- `must_read`: los 21 ficheros existen. Vigente.
- § Contexto, `tm_undo` con `agent` → `tm_command(.., "US-TMC-011")`: vigente (`crates/core/src/channel/conn.rs:4212-4221`). La ruta de `since` (`require_attributed`, `tm_scope_for`, `UndoEnv`, `tm_confirmation`, `undo_selected`): vigente (`conn.rs:4222-4275`).
- `UndoParams::validate` rechaza dos selectores a la vez (`crates/api/src/timemachine.rs:996-1004`), `check_agent_id` (`:1013`), `MAX_SINCE_SECS` (`:22`), `TmRejectReason` termina en `Overlap, OverlapUnverified` (`:245-248`): vigentes. Las tres variantes nuevas van detrás de `OverlapUnverified`.
- **Deriva D-A (adaptación, no choque) — `Selection`**: tras #319 `Selection` es `Last | Since { since_ms } | Within { span_ms }` (`crates/core/src/timemachine/undo.rs:143-152`). `conn.rs` construye `Within` y `undo_selected` la convierte en `Since` con el reloj del daemon **después** del lock y la calma (`undo.rs:1155-1160`, SEC-TMC-07). La spec dice `Agent { selector, since_ms }`. Se aplica la misma regla que ya está en `main`: la variante es `Agent { selector: String, span_ms: i64 }`, y su `since_ms` se calcula en ese mismo punto de `undo_selected`. Así no se duplica la selección del periodo. Sin `--since`, `span_ms` = `MAX_SINCE_SECS`·1000.
- `undo_selected` (`undo.rs:1069`): toma el lock, la calma y `raw_events` antes de planificar. Vigente. El desvío a `undo_agent::plan` va tras la normalización del periodo.
- `UndoStack` (`oplog/stack.rs:49-87`): `done()` existe (de 010). `build` hace `done.retain` en `Undo` y `done.extend(targets)` en `Redo` (`:63-71`). Vigente: es justo lo que T007 cambia. `undo_stack_in` (`:109-160`) reproduce `StackItem::Undo` desde `Target::Undo` del registro. El subtipo **no** pasa hoy a `StackItem`: T007 lo añade al leerlo de `op.record.subtype` (columna existente, `oplog/model.rs:230`, `schema.rs:38`). Sin esquema nuevo.
- `RawGitEvent { seq, kind, branch, actor, observed_ms }` (`engine.rs:67-76`): vigente. **Deriva menor**: hay 8 construcciones que ganan los 4 campos nuevos: `daemon/tm.rs:142` (producción), `timeline.rs:1409`, `capture_mark_tests.rs:490`, `overlap_tests.rs:192`, `undo_since_tests.rs:297`, `undo_since_review_tests.rs:242` y `crates/core/tests/tm_calm_refs_stack.rs:159`.
- `GitEventDetails.{old_commit,new_commit,worktree_inferred}` (`crates/api/src/messages.rs:582-588`) y `Event.gap_id` (`profile/store.rs:236`): vigentes. `daemon/tm.rs::raw_events` ya parsea `GitEventDetails` (`:131`).
- `requester_of` → `sessions::session_actor` (`daemon/tm.rs:156`, `sessions.rs:1048`): vigente. `raw_events` ya lee la atribución vigente en cada llamada (`:122-141`). `EngineLink` no tiene `session_attribution` (`engine.rs:79-120`): se añade con `None` por defecto.
- `AgentFilter::keeps` (`timeline.rs:84-94`) y `operation_actor` con `SessionActors::current_actor` (`timeline.rs:339-354`, `:44-60`): vigentes. `name_matches` se extrae sin cambiar la conducta.
- `RepoReader::changed_paths` y el `diff_trees` privado (`crates/git/src/changed_paths.rs:66`, `:178`), `MAX_PATH_BYTES = 1024` (`:38`): vigentes.
- `ApplyPlan` (`apply/mod.rs:50`), `RefScope` (`:62`), `PlanWorktree.keep` (`:93`), `LoadedWorktree { prior_files, target_files, target_head }` y `is_excluded` (`apply/plan.rs:19-46`): vigentes.
- `overlap.rs`: `check`/`check_with`, `check_tail`/`check_tail_with`, `Subject { Path, Ref }`, `OverlapReport`, `Conflict` (`:34-198`) y `SnapshotStore::changed_between` (`store/mod.rs:612`): vigentes. Como en 010, en el código las funciones son `*_with` (con `events_complete`). `check_selected`/`check_selected_tail` siguen ese patrón.
- **Deriva D-B — `on_remote` (US-TMC-014)**: la ficha está en `draft` y `on_remote` no está en `main`. La enmienda 2026-10-28 (US-TMC-014) de la spec no aplica a este PR.
- **Deriva D-C — fuera de § Estructura**: `apps/cli/tests/protected_process.rs` usa `{ "agent": "claude-1" }` por MCP como ejemplo de "pasa el solicitante y para en su historia" (`NOT_IMPLEMENTED`, commit 82ee5380). Con T008 deja de parar ahí. El ejemplo pasa a `{ "operation_id": "<id válido>" }`, que sigue en `NOT_IMPLEMENTED` ("unassigned"), y conserva la intención del test.
- **Deriva D-D — fuera de § Estructura**: `crates/core/tests/us_tmc_010.rs:142-165` (`since_is_served_and_agent_is_still_not_implemented`) exige que `agent` responda `NOT_IMPLEMENTED` con `implemented_by = US-TMC-011`. T008 lo pone en rojo. Su segunda mitad pasa a `operation_id`, que sigue en `NOT_IMPLEMENTED`; el nombre del test no cambia.
- § 9.3, `Running::agent_work` (`us_tmc_013_common/mod.rs:276`) escribe una operación del oplog a mano, con `recorded_ms = 1`. **No produce commits del motor**. Los escenarios de la spec piden commits reales atribuidos a sesiones, con `old_commit`/`new_commit` y diferencia de árboles. Ver § Arnés de actor.

No hay BLOQUEO: ninguna deriva cambia el diseño.

### Convenciones observadas

- Unitarios en su propio fichero, montados con `#[cfg(test)] #[path = "<f>.rs"] mod <m>;`, como `crates/api/src/lib.rs` (`timemachine_since_tests`), `undo.rs:1371-1380` y `oplog/stack.rs:162-164`. Las líneas de montaje van en el commit de stubs; no las escribe el tester.
- Doble del motor para los unitarios: `undo_since_tests.rs` (`impl EngineLink` + `rig()`). `undo_agent_tests.rs` lo copia y añade `session_attribution`.
- Integración con daemon y Git reales: `us_tmc_013_common::{start, Running, params, rejected, confirm_data, token_of}`, huella `fx.fingerprint()` + `gitraptor_testkit::diff`, confirmación `TestConfirmation::Eligible` (patrón de `us_tmc_010.rs`, `us_tmc_012.rs`).
- e2e: `apps/cli/tests/us_tmc_010_process.rs` y `undo_overlap_process.rs` (`#![cfg(target_os = "macos")]`, `env_clear()` + entorno mínimo, perfil temporal).
- i18n: `apps/cli/i18n/{en,es}/timemachine.txt`, grupo `undo.`, claves al final (G4 de la spec).

### Arnés de actor (independiente de la sesión de Claude Code que corre las pruebas)

- **Agentes simulados**: el daemon de las pruebas se arranca con `ChannelConfig.agents = AgentMatcher::only(vec!["raptor-fake-agent"])` (`crates/core/src/channel/authz.rs:56-79`). Con el override, solo ese nombre de ejecutable es agente (clase Claude Code) y el `claude` real es `Other`. La sesión real que lanza `cargo test` nunca es un agente para estas pruebas.
- **"claude-1" y "claude-2"**: dos procesos `raptor-fake-agent` (copia del binario de test, `gitraptor_testkit::fixture::clone_binary`), cada uno un agente-shell que ejecuta `git` en el worktree. Es el patrón de `crates/core/tests/us_grp_010.rs:250-308` (`agent_in`, `ShellAgent::commit`). El motor les da sesión por ascendencia de proceso. Los ids de sesión se leen de `SESSIONS_LIST` y nunca se inventan.
- **"codex-1"**: un "otro agente" registrado por RPC (`us_grp_010.rs:468`, `register`), con commits desde el shell del test.
- **Corrección y retirada**: `attribution.correct` / retirada por RPC (`us_grp_010.rs:451-459`).
- **Hueco**: el motor ya deja "sin atribuir" lo de un hueco (BR-EDGE-005), así que no hay helper que produzca un evento con hueco **y** sesión. El escenario 4 de integración prueba la conducta observable: motor parado, commits, motor de nuevo, y esos commits siguen tras el undo. La defensa en profundidad (`in_gap` con sesión) la cubre el unitario `a_gap_event_never_enters` con el doble del motor.
- **Dónde vive**: `us_tmc_013_common` arranca con `AgentMatcher` por defecto y está fuera de alcance. El arnés nuevo va en `crates/core/tests/us_tmc_011_common/mod.rs`, del tester: une `start` de `us_tmc_013_common` (operaciones, capa TM, confirmación de test) con el override de agentes y el rol shell de `us_grp_010`. Los e2e usan `GITRAPTOR_AGENT_EXECUTABLES=raptor-fake-agent` (solo debug) con `env_clear()`.
- Todo test que cuente con un dueño comprueba antes su actor en el timeline o en `SESSIONS_LIST`. Si se filtra una atribución ajena, el test falla en vez de pasar en silencio.

### Inventario de reutilización

Solo lo que la spec deja abierto:

| Necesidad | Se reutiliza | Nuevo |
|---|---|---|
| Periodo con reloj del daemon | normalización `Within → Since` de `undo_selected` | `Selection::Agent { selector, span_ms }` (D-A) |
| Rechazo de la petición entera, registro del rechazo | `record_rejection`, `Target::Undo` | — |
| Hash del reto con ids y sujetos | `Confirmation::gate_over` + `PlanFacts::hash_over` | los sujetos con su destino entran en `taken_back` |
| Varios dueños | `restore::restore_permission` | — |
| Precondiciones y ramas en uso | `applier.check_preconditions` (DS-US-TMC-015) | — |
| Fase 2 tras el previo | `UndoStep::check_tail` | rama selectiva en el mismo paso |

## Topología

Componentes, flujo y tareas T001–T011: ver la Dev Spec § Diseño, § La forma y § Plan de implementación. Este Brief no los repite.

Un solo experto (opus), en secuencia: `undo.rs`, `conn.rs` y `stack.rs` los tocan varias tareas.

| Slice | Tareas | Ficheros |
|---|---|---|
| S1 núcleo | T001–T008 | `crates/api/src/timemachine.rs`, `crates/api/src/lib.rs` (montaje); `crates/git/src/changed_paths.rs`; `crates/core/src/timemachine/{engine,agent_selector,timeline,undo_agent,undo,overlap,mod}.rs`, `apply/{mod,plan}.rs`, `oplog/stack.rs`, `daemon/tm.rs`, `channel/conn.rs`; las 8 construcciones de `RawGitEvent`; `apps/cli/tests/protected_process.rs` (D-C); `crates/core/tests/us_tmc_010.rs` (D-D) |
| S2 cli | T009, e2e de T010 | `apps/cli/src/undo/mod.rs`, `apps/cli/src/commands/undo.rs`, `apps/cli/i18n/{en,es}/timemachine.txt` |
| — | T011 | La cierra el worker al final (estado, `release-status`, `xplat-pendientes.md`). Fuera del contrato. |

**Commit de stubs antes de la línea base.** Tras `deliver-run start` y **antes** de `baseline --write`, el experto hace un commit `chore` propio, con los stubs y las líneas de montaje. Es el precedente de 010. Montajes exactos, con el nombre de módulo que usa cada `verify` (cada uno precedido de `#[cfg(test)]`):

- `crates/api/src/lib.rs`: `#[path = "timemachine_agent_tests.rs"] mod timemachine_agent_tests;`
- `crates/core/src/daemon/tm.rs`: `#[path = "tm_attribution_tests.rs"] mod attribution_tests;`
- `crates/core/src/timemachine/agent_selector.rs`: `#[path = "agent_selector_tests.rs"] mod tests;`
- `crates/core/src/timemachine/undo_agent.rs`: `#[path = "undo_agent_tests.rs"] mod tests;`
- `crates/core/src/timemachine/overlap.rs`: `#[path = "overlap_selected_tests.rs"] mod selected_tests;`
- `crates/core/src/timemachine/oplog/stack.rs`: `#[path = "stack_barrier_tests.rs"] mod barrier_tests;`
- `crates/git/src/changed_paths.rs`: `#[path = "changed_paths_tree_diff_tests.rs"] mod tree_diff_tests;`
- `apps/cli/src/undo/mod.rs`: `#[path = "agent_tests.rs"] mod agent_tests;`

Los stubs existen solo para que los tests en rojo compilen. Ninguno lleva `todo!()` ni `unimplemented!()` en una ruta de producción:

- Los tipos y campos de § Contratos compartidos (`SessionAttribution`, `AgentSelector`, `Subject`/`Value`/`OpChange`, `PathTarget`/`SelectiveTarget`, `ApplyPlan.selective = None`, `SelectedInput`, `TreeChange`, los campos nuevos de `RawGitEvent` rellenos con `false`/`None`, las tres variantes de `TmRejectReason` con su texto en la CLI).
- Las funciones devuelven el valor "que falla":
  - `selects`/`names_requester` → `false`;
  - `commit_tree_diff` → `Ok(vec![])`;
  - `is_below_barrier` → `false`;
  - `check_selected*` → `OverlapReport::default()`;
  - `session_attribution` → `None`.
- `Selection::Agent` → `Err(UndoError::Internal("agent: not built"))`.

**En la línea base, cada `verify` de los criterios unitarios debe salir en rojo.** Un verde ahí es un error de montaje (un filtro que no casa con ningún test), no un criterio cumplido.

## Gaps y supuestos

- **G1 (D-A)**: `Selection::Agent` lleva `span_ms`, no `since_ms`. Es la regla de `main` (SEC-TMC-07) aplicada a la variante nueva, no una decisión de diseño.
- **G2**: los tests unitarios de cada tarea van en un fichero propio, no en el fichero que nombra la spec. Así R4 no congela un fichero con código de producción o con tests ajenos. Los nombres de los tests no cambian. Los traslados:
  - T002: `daemon/tm_attribution_tests.rs`, no `daemon/tm.rs`;
  - T003: `agent_selector_tests.rs`, no `agent_selector.rs`;
  - T004: `undo_agent_tests.rs` y `changed_paths_tree_diff_tests.rs`;
  - T005: `crates/core/tests/tm_apply_selective.rs`, no `tm_apply.rs`;
  - T006: `overlap_selected_tests.rs`, no `overlap_tests.rs`;
  - T007: `stack_barrier_tests.rs`, no `stack.rs`;
  - T009: `apps/cli/src/undo/agent_tests.rs`.
- **G3 (D-C, D-D)**: `protected_process.rs` y la segunda mitad de `us_tmc_010.rs::since_is_served_and_agent_is_still_not_implemented` cambian su ejemplo de `agent` a `operation_id`.
- **G5 (escenario 6)**: TMC011-CLI-TEXT comprueba que el texto en/es de `undo.reason.selective-undo-below` nombra las dos salidas, `raptor redo` y `raptor timeline`.
- **G4**: el arnés de integración es nuevo (`us_tmc_011_common`). Sustituye a `Running::agent_work` de § 9.3, que no produce commits del motor.
- **S1**: el escenario 4 de integración no fabrica un evento con hueco **y** sesión, porque el motor no lo produce. Ver § Arnés.
- **S2**: la demo M2 (`m2_demo_undo_agent.rs`) confirma en pty con el arnés de `us_tmc_010_process.rs` / `undo_overlap_process.rs` (`rustix` pty). Sus "4 sesiones de Claude Code" son 4 procesos `raptor-fake-agent`, uno por worktree, con `GITRAPTOR_AGENT_EXECUTABLES`.

### NFR-01: deshacer de menos, nunca de más

- Al conjunto nunca entra trabajo de otro actor, de un hueco o de fuera del periodo. Lo prueban `unattributed_and_other_agents_never_enter`, `a_gap_event_never_enters` y `another_agents_commit_stays`.
- Una operación que no se puede deshacer por separado rechaza la petición entera: `not-selectable`, `raw-git-not-covered` o `target-unavailable`. Lo prueban `an_inferred_worktree_event_is_not_selectable` y `an_excluded_path_in_the_target_is_not_selectable`.
- Un sujeto que otro actor cambió después del último cambio del agente detiene el undo (T006), en fase 1 o en fase 2.
- El aplicador selectivo solo escribe sujetos del conjunto y parte del previo. Todo vuelve con redo.
- La barrera impide que un undo normal devuelva lo quitado.

## Ajuste tras el tester (antes de la aprobación)

- Los `verify` con varios tests usaban `--exact a --exact b`, que libtest rechaza (`Option 'exact' given more than once`). Ahora usan `-- --exact a b`.
- **G6**: TMC011-S6-BARRIER ya no promete en integración la variante "commit de otro actor posterior al último del agente". En la misma rama ese commit detiene antes el undo por agente con `overlap` (D-TMC-34), así que la barrera no llega a probarse ahí. La variante la prueba la pila: `stack_barrier_tests.rs::another_actors_operation_after_the_agents_last_is_below_the_barrier` (TMC011-BARRIER, D6).
- **G7**: TMC011-OVERLAP no puede forzar la fase 1 aparte de la fase 2 en integración, porque `timemachine.snapshot` sigue en `NOT_IMPLEMENTED` y la captura continua está apagada en el arnés. La fase 1 por sujeto la prueba `overlap_selected_tests.rs` (TMC011-OVERLAP-SUBJECT). En integración, `an_uncaptured_edit_just_before_stops_in_phase_2` comprueba el orden `ready → rejected`.
- El arnés `us_tmc_011_common` serializa los mundos con un mutex estático: con daemons en paralelo, el muestreo de `git` del motor perdía atribuciones. El binario tarda unos 150 s.
