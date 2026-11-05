# Brief — `raptor undo --since` (US-TMC-010), preparación

Plan aprobado: [DS-US-TMC-010](../requirements/features/time-machine/dev-specs/US-TMC-010-undo-since.md) (spec-ready, `gaps_blocking: 0`). Este Brief no rediseña nada: contrasta las citas de la spec con el código de hoy (`origin/main` = `HEAD` de `feat/US-TMC-010-undo-since`, 2026-11-04) y fija lo que la spec deja abierto. Contrato: [`us-tmc-010-undo-since.contract.json`](./us-tmc-010-undo-since.contract.json).

## Deriva

Una línea por cita. "Vigente" = el código dice lo que la spec da por hecho.

- `must_read`: los 12 ficheros existen. Vigente.
- § Contexto, `tm_undo` → `tm_command` con `Some("US-TMC-010")`: vigente (`crates/core/src/channel/conn.rs:4053-4064`). El comentario `SECURITY` de `tm_command` está en `conn.rs:4452`.
- § Diseño 2, ruta del undo sin selectores: `require_attributed` + `tm_scope_for` + `UndoEnv` + `tm_confirmation` + `undo_last`: vigente (`conn.rs:4065-4115`).
- `undo_result` rellena desde `UndoDone`: vigente (`conn.rs:3436`). `undo_error` y `Offer.undone_operation_id` llevan un solo `Undone`; con `since` llevan la más antigua (§ T001).
- `UndoParams::validate` / `parse_since` / `MAX_SINCE_SECS`: vigentes (`crates/api/src/timemachine.rs:975`, `:874`, `:22`). `validate` ya rechaza dos selectores a la vez, así que `since` + `agent` da `INVALID_PARAMS` y no llega a la ruta.
- `UndoResult` / `McpUndoResult` / `for_mcp` / `is_zero`: vigentes (`timemachine.rs:439-497`). Llevan `deny_unknown_fields`: un campo nuevo con `serde(default)` no rompe la lectura de respuestas antiguas.
- `RawGitEvent { seq, kind, branch, actor }`: vigente (`crates/core/src/timemachine/engine.rs:67`). **Deriva menor**: hay 5 construcciones, no solo `capture_mark_tests.rs`: `daemon/tm.rs:142` (la de producción; `profile::store::Event.observed.utc_ms` existe, `profile/store.rs:233`), `timemachine/timeline.rs:1409` (test), `timemachine/capture_mark_tests.rs:490`, `timemachine/overlap_tests.rs:192` y `crates/core/tests/tm_calm_refs_stack.rs:159`. Las cinco ganan `observed_ms`; es el "y otros" de § T002.
- `UndoStack` (`oplog/stack.rs:49`): `done` es un campo privado ya ordenado de más antigua a más reciente; `last_operation()` y `next_redo()` existen; `StackItem::Undo { targets }` y la rama `Redo` de `build` ya devuelven todos los objetivos. Vigente. `UndoStack`/`StackScope` se reexportan en `oplog/mod.rs:57`.
- `undo.rs::plan` (`:471`): **deriva de firma, no de diseño**. Tras US-TMC-012 ya recibe `raw_all`, `events_complete` y `deadline`; la spec solo añade `selection`. Hoy usa `stack.last_operation()` y un único `Undone`.
- `undo_last` (`:957`), `UndoDone` (`:127`, `undone: Undone`), `Undone` (`:246`, con `at_ms: Option<i64>` = `recorded_ms` para el oplog y `None` para crudo): vigentes.
- `restore::restore_permission(who, channel, owners)`: vigente (`restore.rs:253`). **Deriva menor**: el undo de hoy no lo usa; llama a `permission()` con un dueño y `confirm.gate`. El hash del plan con los ids del conjunto se hace como en la restauración: `PlanFacts.undone_id` = la más antigua y `confirm.gate_over(base, &facts, &taken_back)` con los demás ids (`confirm/mod.rs:93`, `:220`; `restore.rs:704-741`). `hash_over` ordena los ids (`set()`), así que "en orden" de § T003 es, en el hash, "como conjunto". El efecto pedido se mantiene: una operación más es otro plan.
- `PlanFacts.owners: &[Requester]`: ya es una lista. Vigente.
- Solape de US-TMC-012 (en main): `overlap::check_with` en `plan` (`undo.rs:666`) y `overlap::check_tail_with` en `UndoStep::check_tail` (`:864`). `OverlapInput.reference` y `.undone` ya son slices. **Deriva menor**: `Tail.reference: Requester` y `Tail.undone: OpRef` (`:825`) son singulares y pasan a `Vec`, y `UndoError::Overlap.undone` sigue siendo uno (la más antigua). Los nombres en la spec son `overlap::check`/`check_tail`; en el código, `check_with`/`check_tail_with`.
- Precondiciones DS-US-TMC-015 D7: `applier.check_preconditions(&apply_plan)` antes del previo (`undo.rs:1104-1118`) y otra vez bajo los locks del aplicador. Vigente.
- US-TMC-014 (`on_remote::check`, `UndoResult.on_remote`): **no está en main** (`git grep on_remote origin/main` vacío). La enmienda 2026-10-28 de la spec no aplica en este PR: si 014 aterriza después, la llamada la añade su PR.
- `is_undoable` (`engine.rs:862`) y `raw_target` (`undo.rs:452`): vigentes.
- § Estructura, `apps/cli/src/main.rs`: **deriva menor**. El `Cmd` de clap vive en `apps/cli/src/commands/undo.rs` (la spec ya dice "o el `Cmd` del subcomando"); `undo::run(json_out)` gana `since: Option<String>`.
- § T005, `catalogs_match` (`apps/cli/src/i18n.rs:146`) y `every_rejection_has_a_message` (`apps/cli/src/undo/mod.rs:250`): existen.
- § 9.3, `repo_with_login` (`crates/core/tests/us_tmc_013_common/mod.rs:134`): **deriva**. Crea `main` y `feat-login`, sin `feat-pagos`. El test de escenario 2 añade `feat-pagos` con `r.fx.add_worktree` en su propio fichero; el arnés compartido no se toca.
- § 9.3, `Running::agent_work(session)` (`us_tmc_013_common/mod.rs:276`): **deriva que afecta a las pruebas, no al diseño**. Registra la operación del agente con `recorded_ms = 1` (`record_operation(.., 1)`) y `engine_mark = i64::MAX / 2`: para `--since` queda siempre fuera del periodo, y con D2 el recorrido se pararía en ella. Resolución en § Convenciones (helper local en `us_tmc_010.rs`).
- Brief `tm-prior-point-binding` (#311): el destino de un evento crudo sale de `raw_target` → `raw_candidates` → `prior_point::before_event` (una sola regla, D1 de ese Brief). Con `since`, el destino de la más antigua cruda usa esa misma llamada, sin reglas nuevas. El ancla del undo (`anchor(.., settled)` con `echo_settled`) se escribe una vez para el undo del periodo, como hoy.
- US-GRP-010 (#300, corrección de atribución): `raw_events` lee la atribución vigente en cada llamada (`daemon/tm.rs:122-124`). El dueño de cada evento crudo del conjunto es, por tanto, el corregido en el momento del undo. No hay nada que cachear ni que añadir.
- **Regresión prevista fuera de § Estructura**: `apps/cli/tests/protected_process.rs:226` (`a_declared_identity_changes_nothing`) usa `TM_UNDO { "since": "2h" }` por MCP atribuido como ejemplo de "pasa el solicitante y para en su historia" (`NOT_IMPLEMENTED`). Con T004, `since` sigue la ruta real y la respuesta depende de la capa TM del fixture. Ver G1.

No hay BLOQUEO: ninguna deriva cambia el diseño.

### Convenciones observadas

- Tests unitarios en su propio fichero, montados con `#[cfg(test)] #[path = "<f>.rs"] mod <m>;`: `crates/api/src/lib.rs:34-38` (`timemachine_overlap_tests`), `crates/core/src/timemachine/undo.rs:1240` (`undo_raw_events_tests.rs`), `apps/cli/src/undo/overlap.rs:14-20`. Las líneas de montaje van en el commit de stubs (abajo), no las escribe el tester.
- Doble del motor para los unitarios de `undo.rs`: `undo_raw_events_tests.rs::Engine` (`impl EngineLink`) + `rig()` (repo temporal, `TmRepos`). `since_tests` lo copia y devuelve eventos con `observed_ms`.
- Daemon en proceso: `us_tmc_013_common::start(fx, wt, seam, caps)`, `Running::{client, reset_hard, anchor, recorded, requests, a_rs}`, `params`, `rejected`, `confirm_data`, `token_of`. Huella: `r.fx.fingerprint()` + `gitraptor_testkit::diff` (`us_tmc_013.rs::assert_intact`). Confirmación: `TestConfirmation::Eligible` y la secuencia reto → token (`us_tmc_013.rs::s3_…`, `us_tmc_012.rs::undo_through`).
- Precondición "rebase en curso": `crates/core/tests/us_tmc_002.rs:592` (`an_undo_with_a_git_operation_in_progress_is_rejected`).
- e2e del binario: `apps/cli/tests/undo_process.rs` (`#![cfg(target_os = "macos")]`, `RAPTOR = env!("CARGO_BIN_EXE_raptor")`, daemon en proceso con catálogo de test, `Command::new(RAPTOR).env_clear()`). Entorno: `us_tmc_013_process.rs::raptor_env` (perfil temporal, `HOME` del fixture, `PATH=/usr/bin:/bin`, `LANG`).
- i18n: `apps/cli/i18n/{en,es}/timemachine.txt`, grupo `undo.`; `crate::i18n::has_key` (`i18n.rs:101`) para los tests de claves.

### Aislamiento del actor (las pruebas no dependen de la sesión de Claude Code que las corre)

- **Canal, en proceso**: el cliente `Client::connect` corre en el proceso del daemon. `requester::resolve` corta el recorrido de ancestros al llegar al pid del daemon (`channel/requester.rs:224-235`) y lo da como "sin atribuir". La sesión de Claude Code que lanza `cargo test` está por encima del daemon y nunca se ve.
- **Trabajo de un agente**: siempre se escribe en el oplog con `Requester::Agent { session_id: AGENT_SESSION, .. }` explícito, nunca se detecta. Para `--since` la operación necesita un momento real. `us_tmc_010.rs` lleva su propio helper `agent_work_now(&Running, session)`: la misma secuencia que `Running::agent_work` (campos `pub` `oplog`, `reset_hard()`, `anchor()`), pero con `record_operation(.., now_ms)`. Va registrada **después** de las operaciones sin atribuir del escenario, porque su `engine_mark = i64::MAX / 2` la pone en la cima de la pila. `us_tmc_013_common` no se modifica.
- **Git crudo del test**: lo atribuye el detector por sesiones del worktree temporal, y en él no hay ninguna. Todo test que cuente con un evento crudo comprueba antes que su `actor` es `Unattributed` (vía `requester` del `undone`/oplog, o la respuesta). Si alguna vez se filtra una atribución ajena, el test falla en vez de pasar en silencio.
- **e2e**: `env_clear()` + `raptor_env` (ninguna `CLAUDE_*` ni `HOME` real llega al binario). El binario es hijo del daemon, que corre en el proceso del test, así que se resuelve "sin atribuir". Si el e2e necesitara un agente, `AgentMatcher::only(vec![FAKE_AGENT])` como en `us_tmc_013_process.rs:252`. Los dos e2e de la spec no lo necesitan.
- `DaemonEnv::from_vars(std::env::vars_os())` pasa el entorno del proceso al daemon, y así lo hacen todos los arneses. La atribución no lee variables `CLAUDE_*`: solo usa procesos, sesiones registradas y trailers. No se cambia.

### Inventario de reutilización

Solo lo que la spec deja abierto:

| Necesidad | Se reutiliza | Nuevo |
|---|---|---|
| Lista de `Undone` en `UndoDone` (§ T003, nombre libre) | `Undone` | campo `UndoDone.undone_all: Vec<Undone>` (de la más antigua a la más reciente; `undone` sigue siendo la más antigua) |
| Hash del reto con los ids del conjunto | `Confirmation::gate_over` + `PlanFacts::hash_over` | — |
| Varios dueños en la regla base | `restore::restore_permission` | — |
| Fase 2 del solape con el conjunto | `Tail` | `Tail.reference: Vec<Requester>`, `Tail.undone: Vec<OpRef>` |
| Texto de rechazo con `--since` | `failure_text`, `reason_key` | parámetro `since: Option<&str>` en `failure_text` |

## Topología

Componentes, flujo (mermaid) y tareas T001–T007: ver la Dev Spec § Diseño, § La forma y § Plan de implementación; este Brief no los repite (PC-5).


Un solo experto, en secuencia. Dos slices disjuntos por ficheros. S2 empieza cuando S1 ha terminado T001.

| Slice | Tareas | Ficheros |
|---|---|---|
| S1 core | T001, T002, T003, T004, T006 (core) | `crates/api/src/timemachine.rs`, `crates/api/src/lib.rs` (montaje); `crates/core/src/timemachine/{engine,undo,timeline,capture_mark_tests,overlap_tests}.rs`, `oplog/stack.rs`, `daemon/tm.rs`, `channel/conn.rs`; `crates/core/tests/{us_tmc_010,tm_calm_refs_stack}.rs`; `apps/cli/tests/protected_process.rs` (G1); `crates/core/src/channel/conn_kept_total_tests.rs` (literal `UndoDone`, PC-2) |
| S2 cli | T005, T006 (e2e) | `apps/cli/src/undo/mod.rs`, `apps/cli/src/commands/undo.rs`, `apps/cli/i18n/{en,es}/timemachine.txt`; `apps/cli/tests/us_tmc_010_process.rs` |
| — | T007 | Lo hace el orquestador al cerrar (estado de la historia, `xplat-pendientes.md`). Fuera del contrato. |

**Commit de stubs previo a la línea base** (PC-1, ronda 1 del crítico). Tras la aprobación y `deliver-run start`, y **antes** de `baseline --write`, el experto hace un commit `chore` propio (precedente: 22e8edf9) con los stubs y las líneas de montaje; la línea base no corre hasta que ese commit exista. Montajes exactos, según los filtros del contrato: `#[cfg(test)] #[path = "timemachine_since_tests.rs"] mod timemachine_since_tests;` en `crates/api/src/lib.rs`; `mod done_tests` (→ `stack_done_tests.rs`) en `crates/core/src/timemachine/oplog/stack.rs`; `mod since_tests` (→ `undo_since_tests.rs`) en `crates/core/src/timemachine/undo.rs`; `mod since_tests` (→ `since_tests.rs`) en `apps/cli/src/undo/mod.rs`. El literal `UndoDone { .. }` de `crates/core/src/channel/conn_kept_total_tests.rs:11` gana el campo nuevo en el mismo commit (PC-2). **En la línea base, cada `verify` de TMC010-WIRE, -STACK-DONE, -SELECT, -NFR01-WHOLE, -TARGET y -CLI-TEXT debe salir en rojo; un verde ahí es un error de montaje (filtro que no casa con ningún test), no un criterio cumplido** (PC-3). Stubs para que los tests en rojo compilen. Ninguno lleva `todo!()` ni `unimplemented!()` en una ruta de producción:

- `UndoResult.undone_operation_ids` y `McpUndoResult.undone_count`, con sus `serde`. `for_mcp` pone `0` y `undo_result` deja la lista vacía.
- `RawGitEvent.observed_ms`, relleno en las 5 construcciones.
- `UndoStack::done()`: devuelve `&[]`.
- `undo::Selection`, `undo::undo_selected`: `Last` delega en el camino de hoy; `Since` devuelve `Err(UndoError::Internal("since: not built"))`.
- `UndoDone.undone_all`.
- Claves i18n vacías: no. Las claves las añade T005 de verdad.

## Gaps y supuestos

- **G1 (fuera de § Estructura)**: `apps/cli/tests/protected_process.rs:226` dejará de dar `NOT_IMPLEMENTED` con `since`. Propuesta: cambiar el selector del ejemplo a `{ "agent": "claude-x" }`, que sigue en `NOT_IMPLEMENTED` (US-TMC-011) y conserva la intención del test (pasa la comprobación del solicitante y para en su historia). El build lo confirma ejecutándolo; si sigue verde, no se toca.
- **G2**: `since_equals_consecutive_undos` compara dos repos temporales distintos, así que sus huellas no se comparan tal cual (las rutas difieren). Se comparan el contenido de los ficheros del worktree, el árbol de `HEAD` y las puntas de las ramas.
- **Supuesto S1**: el reto de un periodo muestra en `TmConfirmData.undone_operation_id` la operación más antigua, y en `owners`, todos los dueños (hasta `MAX_CONFIRM_OWNERS`). La CLI de confirmación no cambia (§ T005).
- **Supuesto S2**: el escenario 1 espera por reloj a que `recorded_ms` de A tenga más de 10 s: es la única espera por tiempo (§ T006). Se espera a la condición (`now − recorded_ms > 10 000`), no un `sleep` fijo a ciegas.
- **Supuesto S3**: un rechazo de `since` registra `Target::Undo(conjunto)` y, sin conjunto, `Target::Undo([])` (§ T003).

### NFR-01: deshacer de menos, nunca de más

- El conjunto es el sufijo de la pila con momento ≥ umbral, y el recorrido para en la primera operación más antigua (D2). Con un reloj desordenado se deshace de menos.
- Si una operación del conjunto no se puede deshacer (evento crudo con `is_undoable` falso, operación manipulada o ilegible, destino no disponible), se **rechaza la petición entera**: `raw-git-not-covered` / `target-unavailable`, la huella no cambia y el rechazo queda en el oplog (D4, D-TMC-29). Nunca se salta en silencio.
- El trabajo de otro actor del periodo nunca se excluye. Con un agente como solicitante, la respuesta es `other-actor`. Sin atribuir, pide confirmación con el reto ligado a los ids del conjunto. Por MCP o en Windows, se rechaza.
- Lo que se apoya en esto: `a_raw_event_not_covered_rejects_the_whole_period`, `since_stops_at_the_first_older_operation`, `since_never_touches_other_worktrees` y `another_actor_in_the_period_without_confirmation_is_rejected`.

Cuestión de coste: la spec obliga a 9 tests de integración y a 2 e2e. Con unitarios sobre `undo_selected` y 4 de integración se cubrirían los mismos caminos más barato. Se construyen todos, como pide la spec.
