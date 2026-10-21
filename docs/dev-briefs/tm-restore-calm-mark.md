# Brief: la marca en calma espera a los refs (fix/tm-restore-calm-mark)

## Alcance

NFR-01. `us_tmc_009::a_restore_can_be_undone_back_to_the_exact_prior_state` falla en CI: el undo deshace `git-event-3` (eco tardío del commit protegido) en vez del restore. Este run cierra el hueco de la calma del motor para que toda marca en calma (intención, ancla, restore, undo, captura) cubra también los cambios de refs ya hechos en disco. Cita ADR-TMC-003 § 4 (Marca en calma, `caused_by` por ancla) y ADR-TMC-004 § 2 (calma por estado). Validado por nassa-architect «con ajustes»; los ajustes ya están incorporados.

### Causa raíz (evidencia)

- La calma es **solo por el reflog de `HEAD`** del worktree: `timemachine/engine.rs:RepoMarks::calm` (L159-189). Ningún ref de rama entra en la comparación.
- Hueco H1 (commit a medias): `git` añade al reflog de `HEAD` antes de renombrar el ref. `watch/repo.rs` L816-833 difiere una sola vez un lote "solo reflog" y después lo envía **sin eventos**. Además `RefsView::read` (L84-120) lee las ramas antes que `head_log_len` (L216). `daemon/repos.rs:observed` L497-501 publica esos `head_logs`, así que `settle` da una marca que no cubre `commit`/`branch-update` (eventos 2 y 3). Con eso, el ancla de la op 15 (`channel/conn.rs:3330 after_step`) y la intención del restore (`restore.rs:435-438`) toman la marca 1.
- Hueco H2 (refs fuera de `HEAD`): `branch -D` y el `update-ref` del aplicador no escriben el reflog de `HEAD`, así que la calma es inmediata. Eso explica la op 15 con marca 0. `continuous.rs:await_moved_branches` (L667-718) solo cubre ramas checked-out, y le basta *cualquier* evento posterior con el mismo nombre de rama (falso positivo: los 2 y 3 de la op 15 "satisfacen" la espera del restore).
- Resultado: `undo.rs:external_events_in` (L274-335) deja 2 y 3 sin `caused_by`. `oplog/stack.rs:undo_stack_in` (L110-112) los ordena por `seq` encima del restore (marca 1).

### Decisiones

Cada una es una «Decisión del orquestador (2026-10-04), validada por Arquitecto».

- **D1. Barrera por estado de refs, no ligado de ecos.** Un worktree está en calma solo si, además de lo de hoy, las ramas locales del repo en disco son las del último lote persistido. Ligar ecos exigiría adivinar qué emite un `git`. La barrera cierra H1 y H2 y mantiene "calma por estado" (ADR-TMC-004 § 2).
- **D2. Paridad de lectura.** `RefsView::read` y la calma leen las ramas con un único helper, `watch::read_local_branches`, sobre `RepoReader::local_branches`. La comparación es directa entre `BTreeMap<String, String>`, sin construir texto.
- **D3. Directorio común confiable.** `RepoMarks` guarda el directorio común que registró el daemon y lee de ahí, nunca a través del `.git` de un worktree (#223 I-03).
- **D4. Falla cerrado.** La lectura es falible (`Result`). Si el repo no abre o el listado falla, la respuesta es "no en calma" (no se usa el `unwrap_or_default` de `continuous::branch_tips`). Solo si el motor nunca publicó ramas para el repo se usa el respaldo por tiempo `quiet()`. Fuera del límite, `RepoBusy`/`GitBusy`, como hoy.
- **D5. Orden de memoria explícito.** Ramas y `seq` del último lote viven en el mismo `Mutex` (`RepoMark.persisted`). El escritor publica `seq` y ramas juntos, en una sección crítica. El lector compara las ramas y lee `seq` en esa misma sección. Si coinciden con las del lote N, la `seq` devuelta es ≥ la del lote N. El `AtomicI64 seq` se sigue actualizando para `mark()`.
- **D6. Barrera por repo.** Se comprueba una vez por sondeo de `settle`, para todo el repo, no por worktree. Los commits de un worktree B retrasan el `settle` de A. Es un límite declarado y medido (C6). No se acota por worktree en este fix.
- **D7. Despertar antes de esperar.** `DaemonEngine::settle` envía `ShutdownHandle::wake(repo_id, WakeCause::Sentinel)` antes de sondear. Hoy el restore y el undo solo despiertan el repo en `raw_events` (`daemon/mod.rs:830-839`), que va *después* de `settle`, y el ejecutor no lo despierta.
- **D8. `await_moved_branches` se mantiene sin cambios en este fix.** Se retira aparte (ver Not Built).

## Topología

Slice 1 (tracer, criterios: C1, C2, C3, C3b):
- `crates/core/src/watch/mod.rs`: `pub fn read_local_branches`. `WatchStart.branches: Option<BTreeMap<..>>`, que se rellena en `watch_repo` (L988) y al despertar (L1219).
- `crates/core/src/watch/repo.rs`: `RefsView::read` usa `read_local_branches` (el error se mapea a `RefsReadError::Listing`).
- `crates/core/src/timemachine/engine.rs`: `RepoMark.persisted` y `common_dir`, `set_persisted`, `refs_calm` dentro de `settle`, `remove()` que limpia todo. Doc del módulo (refs y orden de memoria).
- `crates/core/src/daemon/repos.rs`: `set_persisted` tras `write_batch` Ok (L499-501) y en el arranque (L355). Las ramas del lote viajan en un campo nuevo `ObservedBatch.branches: Option<BTreeMap<..>>`, que sale de `new.branches` en `repo.rs` L801.
- `crates/core/src/daemon/tiers.rs`: `set_persisted` al despertar (L352).
- `crates/core/src/daemon/tm.rs`: D7 en `DaemonEngine::settle`.
- `crates/core/tests/tm_calm_refs.rs` (nuevo): C1, C1b, C2, C3, C3b, C5b, C8.

Slice 2 (criterios: C4, C6, C7, C9):
- `crates/core/tests/tm_calm_refs_stack.rs` (nuevo): C4.
- `crates/core/tests/tm_calm_refs_load.rs` (nuevo, `#[ignore]`): C6, C9.
- `crates/core/tests/tm_calm_dormant.rs` (nuevo): C7.
- `docs/architecture/decisions/ADR-TMC-003-…md`: enmienda. `ADR-TMC-004-…md`: una línea en la fila "Calma por estado".

## Contratos

- `pub fn read_local_branches(reader: &RepoReader) -> Result<BTreeMap<String, String>, RefsReadError>` en `gitraptor_core::watch`. `RefsReadError` pasa a ser `pub` en `watch`.
- `pub fn set_persisted(&self, repo_id: &str, seq: i64, common_dir: &Path, branches: BTreeMap<String, String>)` en `RepoMarks`. Hace en un solo lock `seq.fetch_max(seq)` + `persisted = Some((seq, branches))` y guarda `common_dir`. En el daemon sustituye al `set_seq` posterior a cada lote que trae ramas.
- `RepoMarks::settle(&self, repo_id, worktrees, limit) -> Option<i64>`: misma firma. Devuelve `Some(s)` solo si `all(calm)` y `refs_calm` dan `Some(s)`. `refs_calm`, bajo el lock de `persisted`:
  - sin publicar: `quiet().then(seq)`;
  - con publicado `(s, b)`: abre el directorio común; si la lectura da `Err`, `None`; si las ramas en disco `== b`, `Some(max(s, seq atómico))`; si no, `None`.
- `RepoMarks::remove(repo_id)`: borra también `persisted` y `common_dir` (sale con la entrada entera del mapa; un test lo fija).
- `ObservedBatch.branches: Option<BTreeMap<String, String>>`, `WatchStart.branches: Option<BTreeMap<String, String>>`: campos nuevos, con `None` por defecto.
- `EngineLink`, el wire y los códigos `repo-busy`/`git-busy`: sin cambios.

## Criterios (tests; repos temporales con `HOME`/`GIT_CONFIG_GLOBAL` aislados, sin `sleep`)

- C1 `settle_waits_for_a_ref_move_the_engine_has_not_persisted`: se publica `(1, ramas)`, se inyecta `git update-ref refs/heads/x <c>` y se afirma `settle(ZERO) == None`. Después se publica el lote tardío `(3, nuevas)` y se afirma `Some(3)`. En rojo.
- C1b `packing_refs_between_the_batch_and_settle_keeps_the_calm`: se publica, se hace `git pack-refs --all` y se afirma `Some`.
- C2 `a_heads_only_batch_does_not_calm_a_commit_whose_ref_is_unpersisted`: `git commit` en un worktree enlazado. `head_logs` lleva el tamaño nuevo y las ramas publicadas son las viejas: `None`. Tras publicar las nuevas: `Some`. En rojo.
- C3 `settle_without_published_refs_keeps_the_quiet_rule`: sin publicar y con `touch(now)`, `None` a 30 ms. Con `touch(0)`, `Some`.
- C3b `an_unreadable_common_dir_is_never_calm`: se publica con ramas vacías, se borra o corrompe el directorio común y se afirma `settle(30 ms) == None`. En rojo.
- C4 `a_late_echo_bound_by_a_calm_anchor_stays_below_the_restore`: `RepoMarks` real. El lote tardío (seq 2 y 3) se publica en el orden inyectado. Se afirma que el ancla de la op 15 y la marca del restore son ≥ 3, que `caused_by(2,3) = op15` (`external_events_in`) y que `undo_stack_in(..).last_operation() == restore`.
- C5 (e2e) `us_tmc_009::a_restore_can_be_undone_back_to_the_exact_prior_state` sin cambios: 50 de 50 bajo carga en ubuntu.
- C5b `remove_forgets_the_persisted_refs`: tras `remove`, se vuelve a la regla `quiet()`.
- C6 `#[ignore] settle_holds_under_another_worktrees_ref_churn`: el worktree B mueve una rama cada X ms con un daemon real y el `settle` de A debe dar `Some` dentro de `ANCHOR_SETTLE_LIMIT`. Barre X e imprime la X mínima.
- C7 `a_tm_operation_wakes_a_dormant_repo_before_settling`: con el fixture de tiers del daemon, se duerme el repo, se mueve una rama mientras duerme y se lanza un undo. Debe salir `Ok`, no `repo-busy`.
- C8 `a_corrupt_loose_ref_reads_the_same_in_the_view_and_the_calm`: basura en `refs/heads/roto`. `RefsView::read` y `refs_calm` dan el mismo resultado (los dos `Err` → no en calma).
- C9 `#[ignore] settle_is_cheap_when_idle`: p95 de `settle` en reposo menor de 5 ms en release. Se mide también con unas 5000 ramas e imprime los números; no es un gate de debug.

## Convenciones observadas

- Marcas: primero `seq` y después lo leído (`daemon/repos.rs:observed` L497-501).
- Lectura del directorio común (`continuous.rs:branch_tips`) y errores de vista (`watch/repo.rs:RefsReadError`).
- Tests de calma con `tempdir` y límites de `ZERO`/30 ms (`engine.rs:tests::a_worktree_is_calm_when_the_engine_read_its_head_reflog`).
- Tests de contrato en su propio archivo (R4). Git aislado como en `tests/us_tmc_009.rs:git`.

## Inventario de reutilización

- Reutiliza: `RepoReader::{open,local_branches}`, `RepoMarks::{set_head_logs,set_heads,touch,settle,remove}`, `ShutdownHandle::wake`, `external_events_in`, `Oplog::undo_stack_in` y el fixture de tiers de los tests del daemon.
- Genera: ninguno.
- Crea nuevo: `read_local_branches` (code-search "local_branches helper refs view": solo el bucle inline de `RefsView::read`), `RepoMarks::set_persisted` (code-search "persisted branches marks": nada).

## Supuestos

- Packed-refs: `local_branches` de gix lee loose y packed, así que `git pack-refs` no cambia el mapa (C1b). El backend reftable sigue siendo un límite declarado (ADR-TMC-004).
- Solo cuentan las ramas locales. Los remotos y los tags no son eco de una operación TM.
- Repo dormido: hoy `settle` corre antes de cualquier despertar (`restore.rs:435`, `undo.rs:732`, `conn.rs:3323`), y D7 lo corrige. El despertar es asíncrono, así que `settle` espera al primer lote del repo despierto dentro del límite. Si no llega, `repo-busy` (C7).
- Coste: una apertura más un listado por sondeo de 10 ms solo mientras espera; la comparación es de `BTreeMap`. Presupuesto en C9.
- Plataformas: misma lógica en Linux, macOS y Windows. CI en ubuntu; macOS y Windows quedan pendientes en máquina real.
- Seguridad: no hay rutas nuevas desde el contenido del repo y los nombres de rama solo se comparan. Va a `security-expert` (NFR-01).

### Enmienda ADR-TMC-003 § 4 (esquema)

"Enmienda (2026-10-21, fix/tm-restore-calm-mark)":
1. La Marca en calma exige además que las ramas locales del directorio común sean las del último lote persistido, comparadas bajo el mismo lock que su `seq`.
2. Por qué: el reflog se escribe antes que el ref, y `branch -D` y el aplicador no escriben el reflog.
3. Falla cerrado si el repo no se lee.
4. Límite declarado: la barrera es por repo. Con otro worktree moviendo ramas cada menos de X_min ms (medido en C6, se anota el valor), el `settle` puede agotar 2 s y dar `repo-busy`.

ADR-TMC-004 § 2: se añade "y las ramas locales" a la fila de Calma por estado.

## Decisiones abiertas

Ninguna. No hay cambio de contrato público ni de datos.

## Not Built (deferred)

- Barrera por worktree (solo las ramas de su ámbito): cuando C6 muestre `repo-busy` con la cadencia real de los agentes.
- Retirar `await_moved_branches`: cuando C4 y us_tmc_009 lleven 2 semanas en verde. Su falso positivo (cualquier evento ajeno posterior con el mismo nombre de rama) no puede sobrevivir a la retirada, ni copiado ni en un sustituto.
- Calma con el backend reftable, y remotos y tags en la barrera: cuando se soporten o una operación TM los mueva.

## Stubs (para que compilen los tests en rojo)

- `pub fn read_local_branches(reader: &RepoReader) -> Result<BTreeMap<String, String>, RefsReadError> { Ok(BTreeMap::new()) }` en `watch/mod.rs`, con `RefsReadError` como `pub`.
- `pub fn set_persisted(&self, _repo_id: &str, seq: i64, _common_dir: &Path, _branches: BTreeMap<String, String>) { self.set_seq(_repo_id, seq) }` en `RepoMarks`.
