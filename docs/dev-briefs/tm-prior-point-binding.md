# Brief — Punto previo ligado a su comando (NFR-01, `fix/tm-prior-point-binding`)

## Alcance

El punto "antes de un evento" del timeline (`Points::before_event`) y el destino del undo de Git crudo (`undo::raw_target`) pueden elegir una foto tomada a mitad de operación o una foto con la marca atrasada (W85, 2026-10-21: 39 % bajo carga en `us_tmc_005_hook::scenario_1_rebase_entry_shows_hook_prior`). Este fix unifica la elección en una sola función pura, liga cada previo a los eventos de su propio comando y marca en el oplog las observaciones tomadas a mitad de operación. Corrige ADR-TMC-003 § 4 ("Git crudo en la pila", destino) y D10 de `us-tmc-005-pre-hook-snapshot.md`.

**Qué llevan los eventos del motor**: `RawGitEvent { seq, kind, branch, actor }` (`engine.rs`). No llevan el proceso `git` (el detector muestrea procesos, pero no guarda `(pid, start_us)` por evento). Por eso el enlace por proceso no es posible sin cambiar el almacén del motor (D2).

## Decisiones

Todas las D: «Decisión del orquestador (2026-10-04), validada por Arquitecto». nassa-architect las validó con los ajustes 1 a 7 del 2026-10-21.

- **D1. Una sola regla.** `prior_point::before_event` es la única que elige el punto previo. La usan `timeline::Points` y el undo (`raw_candidates`, y `raw_target` verifica en el almacén por orden). Se elimina el orden por rango de D10.
  - **Entrada:** recibe **todos** los puntos con marca de la clave, también los previos no disponibles o alterados. La sombra se calcula sobre ese conjunto completo.
  - **Filtro:** el de disponibilidad se aplica después, sobre los índices devueltos.
    - Undo: `state.is_available() && !tampered`, y luego `store.verify`.
    - Timeline: ese mismo filtro, y además estar entre los puntos ofrecibles de `OplogRead.points`.
  - El timeline y el undo parten del mismo conjunto sin filtrar (`OplogRead.all_snapshots` / `oplog.snapshots`).
  - Un previo perdido sigue ensombreciendo. Si no queda nadie, gana un punto más antiguo o `TargetUnavailable`.
- **D2. Enlace por ventana de eventos del worktree, no por proceso.**
  - **Ventana:** para cada previo `P` (`HookPrior` o `GuaranteedPrior`) de la clave `k`, `f_P` es el primer evento crudo de `k` con `seq > P.mark` (∞ si no hay). Para un `GuaranteedPrior`, la ventana termina también en su ancla con `echo_settled` (Enmienda 2026-10-28, más abajo).
  - **Sombra (enmienda 2026-10-21, regresión de `tm_chaos::hostile_half_done_merge_rejects_until_aborted`):** un punto `Q` de `k` queda **ensombrecido por `P`** si `Q.seq > P.seq` y se cumple (a) o (b).
    - **(a) Marca atrasada:** `Q.mark < f_P`. Se registró después del previo, pero antes de que el motor persistiera el primer evento de ese comando, así que su foto puede contener ya el efecto del comando con una marca atrasada.
    - **(b) Tramo a mitad de operación tras el previo:** `Q.mid_operation` y no existe ningún punto limpio `R` de `k` con `P.seq < R.seq < Q.seq`. "Limpio" es un previo, o un punto con `mid_operation != Some(true)` que `P` no ensombrece por (a) (`R.mark >= f_P`). `R` puede estar o no disponible, como en D1.
    - Un punto ensombrecido nunca es "antes de un evento".
  - **Alcance:** se aplica a todos los niveles, también a un previo posterior, y por clave: los eventos de otro worktree no mueven `f_P`.
  - **`floor` (ajuste 2):** decide quién puede ganar, no quién proyecta sombra. Un previo de la generación actual proyecta sombra aunque no sea disponible.
  - **Desviación consciente del ajuste 2:** un previo con `seq < floor` no ensombrece puntos con `seq >= floor`.
    - Sus marcas son de otra numeración (ADR-TMC-003 § 4, "Generación"), y `f_P` contra eventos de la generación nueva sería ∞.
    - Así ensombrecería todo lo posterior, y cada cambio de generación dejaría el undo en `TargetUnavailable`.
    - Entre puntos bajo `floor` sí ensombrece, aunque ninguno de ellos puede ganar.
- **D3. Observación a mitad de operación.** Toda captura completa guarda `mid_operation = en curso al inicio || en curso al final` (`RepoReader::in_progress` en alguno de sus worktrees, antes de leer y otra vez en el punto de validez).
  - **Exclusión (enmienda 2026-10-21):** ya no es incondicional. Una foto a mitad de operación solo queda fuera por la sombra (b) de D2: cuando sigue a un previo sin ningún punto limpio en medio.
    - **Caso hook:** es el del rebase con hook, donde el comando que dejó la operación a medias es el del previo.
    - **Sin previo, o con un punto limpio en medio:** la foto a mitad sigue siendo elegible. Es el "antes" correcto de un `merge --abort` (`Reset`), porque guarda las resoluciones parciales. `tm_chaos::hostile_half_done_merge_rejects_until_aborted` restaura `stopped`, como antes del fix.
  - **Previos:** un previo nunca se excluye por `mid_operation` (D9 de US-TMC-005 sigue vigente). Lo toma con `rebase-merge/` ya presente (`--autostash`) y es la foto anterior por construcción.
  - Las fotos siguen en el timeline como puntos propios.
  - **Combinación:** la hace `prior_point::mid_operation(at_start, at_end)`.
  - **Qué se prueba:** una operación que empieza durante la captura ya la descarta casi siempre la guarda `GitState` (HEAD o índice cambian). Por eso, el caso "en curso solo al final" se prueba sobre la función, no de extremo a extremo.
- **D4. Sin migración.** `mid_operation` va en `CompleteInfo` (JSON del `detail` de la entrada `complete` del journal, que ya está encadenada). Usa `#[serde(default, skip_serializing_if = "Option::is_none")]` y es `Option<bool>`. `None` (filas anteriores al fix) cuenta como "no a mitad de operación", que es el comportamiento de hoy. No hay `OPLOG_MIGRATIONS` nuevo y los hashes no cambian.
- **D5. Orden.** Entre los elegibles (`mark < seq`, `seq >= floor`, sin sombra (a) ni (b)), gana el mayor `(mark, seq)`. Con la misma marca, un previo registrado después de una observación gana por `seq`: es el caso de D10, que sigue cubierto.
- **D6. Fallar cerrado = más antiguo.** Nunca se elige una foto a mitad del comando de un previo, ni una con la marca atrasada, salvo el caso sin previo declarado en Not Built (`TRADEOFF`). Si hay duda, se elige un punto anterior. Restaurar de más siempre se recupera (el undo toma su propio previo garantizado, NFR-01), mientras que una foto a mitad de operación o posterior al evento pierde el estado previo. Ver `TRADEOFF` en Not Built.
- **D7. Documentación.**
  - **ADR-TMC-003 § 4:** se enmienda la fila "Git crudo en la pila" con el destino de D1, D2, D3 y D5. Se documenta `mid_operation` como parte del esquema del `detail` de la entrada `complete`. Se registra como límite abierto de NFR-01 el bug de pila de us_tmc_009 (`external_events_in` liga el eco solo por la marca del ancla; PR #256).
  - **ADR-TMC-004:** se añade una fila a su tabla de enmiendas: la captura registra `mid_operation`, y una observación a mitad de operación que sigue a un previo sin un punto limpio en medio ya no es un punto "antes de un evento" (D2 b).
  - **`us-tmc-005-pre-hook-snapshot.md`:** D10 se marca **sustituida** por `tm-prior-point-binding` D1/D5. Se anota en D9 que sigue vigente: un previo nunca se descarta por `mid_operation`.

**Cierra el bug de us_tmc_009 (`a_restore_can_be_undone_back_to_the_exact_prior_state`)? No.** Ese fallo está en la **pila**: el evento tardío 3 cae fuera del eco del restore (`external_events_in`, el ancla con una marca anterior a la del evento), y `last_operation` es `GitEvent(3)`. Este fix no cambia el eco. Solo arregla el **estado** al que lleva ese undo: con D2, el ancla queda ensombrecida por el previo garantizado del restore, y deshacer el evento 3 restaura el estado exacto anterior al restore, no el ancla. La aserción `undone_operation_id == restore.operation_id` puede seguir fallando.

## Enmienda 2026-10-28 (NFR-01: edición tras restore)

**Decisión del orquestador (2026-10-28), validada por Arquitecto.** Rama `fix/nfr01-uncommitted-capture-window`. Enmienda la ventana de D2 para el `GuaranteedPrior`.

**Bug.** Tras un `raptor restore` o `raptor undo` (previo garantizado y ancla con el mismo `cause_operation`), la observación continua capta una edición ~1 s después. Si sigue un `git reset --hard` crudo que no mueve `HEAD`, `f_P` es ese reset (el restore no movió ningún ref): la observación de la edición queda ensombrecida por (a), y el timeline y el undo ofrecen el previo del restore, sin la edición. Reproducido en un repo temporal; evidencia de Rene: se ofreció el previo `aea6241b`/`5d254fb9` en vez de la edición.

**Cambio.**
- `CompleteInfo.echo_settled: Option<bool>` en el ancla de una operación, con `serde(default)` y sin migración (como `mid_operation`, D4). Vale `true` cuando `await_moved_branches` (ahora devuelve `bool`) confirmó todas las ramas que movió la operación antes de `ANCHOR_SETTLE_LIMIT`, también si no movió ninguna. Lo escriben `restore.rs` y `undo.rs`. El ancla del ejecutor (`channel/conn.rs`, `after_step`) escribe `false`: no espera las ramas movidas.
- `prior_point::settled_anchors` liga cada operación a su primera ancla de observación con `echo_settled == Some(true)`, y `PointFacts.anchor_seq` la lleva para un `GuaranteedPrior`.
- En `before_event`, un punto registrado después de esa ancla (`seq > anchor_seq`) no queda ensombrecido por ese previo: el eco de la operación ya está bajo la marca del ancla, así que la ventana de su comando terminó. El ancla sigue ensombrecida (C2 sin cambios).
- `None` (filas anteriores) y `Some(false)` mantienen la ventana hasta `f_P`. `HookPrior` sin cambios: el fin de su comando no se conoce (`TRADEOFF` de Not Built).

**C9 — edición tras un restore.**
- `prior_point_tests.rs`: `an_edit_captured_after_a_restore_is_the_point_before_a_raw_reset`, `a_restore_with_a_late_git_event_still_shadows_its_anchor` y `a_hook_prior_window_still_runs_until_its_first_event`.
- `timeline_prior_point_tests.rs`: `timeline_and_undo_pick_an_edit_made_after_a_restore_for_a_raw_reset` y `an_anchor_that_does_not_certify_its_echo_keeps_the_prior` (`Some(false)` y `None`).
- `apps/cli/tests/restore_process.rs`: `an_edit_after_a_restore_survives_a_raw_reset_hard` (binario `raptor` real y daemon).

**Límites residuales.**
- **Ventana de la captura continua** ≈ Q (1 s) + tiempo de captura: una edición seguida de un `reset --hard` crudo antes de ~1 s se pierde. Ningún hook de Git corre antes de que un `reset --hard` sin movimiento sobrescriba el worktree (verificado con git 2.50.1: solo `post-index-change` y `reference-transaction` de `ORIG_HEAD` y de la rama con el mismo valor viejo y nuevo, los dos después de sobrescribir).
- **Operaciones del ejecutor** (MCP, Cockpit): conservan el comportamiento anterior hasta que su ancla espere las ramas movidas (seguimiento).
- **Previo de hook de un comando sin efecto**: `TRADEOFF` existente de Not Built.

## Contratos

- Nuevo `crates/core/src/timemachine/prior_point.rs` (`pub(crate) mod prior_point;` en `timemachine/mod.rs`):
  - `pub(crate) struct PointFacts { pub seq: i64, pub mark: i64, pub prior: bool, pub mid_operation: bool }`, con `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.
  - `pub(crate) fn before_event(points: &[PointFacts], event_seqs: &[i64], seq: i64, floor: i64) -> Vec<usize>`.
  - `points` son los puntos de una clave con marca, en cualquier orden. `event_seqs` son los `seq` crudos de esa clave, ascendentes.
  - Devuelve los índices elegibles por D2, D3 y D5, del mejor al peor, sin mirar la disponibilidad, porque esa la filtra el llamador (D1). Es puro, sin I/O y sin pánico.
  - `pub(crate) fn mid_operation(at_start: bool, at_end: bool) -> bool` (D3).
- `oplog::CompleteInfo`: `pub mid_operation: Option<bool>` (D4).
- `timeline::Points`:
  - `with_events(all: &[SnapshotView], events: &HashMap<String, Vec<i64>>) -> Points`: `all` son **todos** los snapshots; `events` va de la clave de snapshot a sus seqs.
  - `pub(super) fn offerable(self, points: &[SnapshotView]) -> Self`: restringe los ganadores a esos ids.
  - `before_event(&self, key, seq, floor)` conserva su firma. Devuelve el primero de D1 que sea disponible, no alterado y ofrecible.
  - `Points::new` se elimina: `build_timeline` hace `with_events(&read.all_snapshots, …).offerable(read.points)`, y arma `events` desde `EngineSide.raw` y `snapshot_keys`.
- `undo`:
  - `pub(super) fn raw_candidates(snaps: &[SnapshotView], wt_key: &str, event_seqs: &[i64], seq: i64, floor: i64) -> Vec<String>`: pasa **todos** los puntos de la clave a D1 y luego filtra los índices devueltos por `state.is_available() && !tampered`.
  - `raw_target(oplog, store, wt_key, event_seqs, seq, floor)` devuelve el primero de `raw_candidates` que pase `store.verify`. El llamador pasa `raw.events`.
- `store::capture`: al completar, escribe `mid_operation: Some(mid_operation(start, end))`. `in_progress` se lee al inicio y en el punto de validez, con el lector registrado que ya abre (#223 I-03).
- Errores y wire: sin cambios (`TargetUnavailable` si no hay candidato verificado).

## Criterios

Los tests C1 a C5 están en `prior_point_tests.rs`; los de C6, en `timeline_prior_point_tests.rs` (`mod prior_point_wiring`); los de C7, en `tests/tm_prior_point_condition.rs`.

- **C1 — rebase con observación tardía.** Situación: `HookPrior(m0)`, `Obs(m0)` anterior, evento 1, `Obs(m1)` con `mid_operation` y evento 2. Para los eventos 1 y 2 se elige el previo. Sin previo, se elige `Obs(m0)`.
  - `rebase_with_a_late_mid_operation_observation_picks_the_hook_prior`
  - ~~`rebase_without_a_prior_skips_the_mid_operation_observation`~~ queda **sustituido** por la enmienda de D3: sin previo, la foto a mitad es elegible y el test fallaría. Lo reemplaza `rebase_without_a_prior_keeps_the_mid_operation_observation_eligible`: con los mismos puntos, el evento 12 elige `mid(2,11)` y el 11 elige `obs(1,10)`. Es una limitación declarada (`TRADEOFF`).
- **C2 — restore con evento Git tardío.** Situación: `GuaranteedPrior(m2)`, ancla `Obs(m2)` posterior y evento 3. Se elige el previo, no el ancla. Control: con el ancla en `m3`, el evento 4 elige el ancla.
  - `restore_with_a_late_git_event_picks_the_guaranteed_prior_not_the_anchor`
  - `restore_anchor_whose_mark_covers_the_event_is_picked_after_it`
- **C3 — borrado de rama con previo.** Una observación posterior a la marca del previo, con la misma marca o con la marca subida por otro worktree, no gana al previo para el evento `BranchDelete`.
  - `branch_deletion_with_a_late_observation_picks_the_hook_prior`
  - `branch_deletion_with_an_interleaved_mark_bump_picks_the_hook_prior`
- **C4 — dos worktrees intercalados.** Cada previo se liga solo a los eventos de su clave, y un evento de otra clave no cierra la ventana.
  - `interleaved_worktrees_bind_each_prior_to_its_own_events`
  - `interleaved_events_of_another_worktree_do_not_close_the_window`
- **C5 — controles negativos.** Un punto limpio con `mark >= f_P` gana para el evento siguiente. Nunca se elige un punto por debajo del suelo ni uno con `mark >= seq`.
  - `control_a_clean_point_after_the_command_is_picked_for_the_next_event`
  - `control_points_below_the_floor_or_not_before_the_event_are_never_picked`
  - *(nuevo)* `control_a_prior_below_the_floor_casts_no_shadow_on_the_current_generation`: D2, desviación del ajuste 2.
- **C6 — timeline y undo eligen lo mismo**, y el undo nunca apunta a una observación a mitad de operación ensombrecida por (b).
  - `timeline_and_undo_pick_the_same_point_for_an_event`
  - ~~`undo_never_targets_a_mid_operation_observation`~~ queda **sustituido**: no tiene previo, así que ahora elegiría `mid`. Lo reemplaza `undo_never_targets_a_mid_operation_observation_that_follows_a_prior`, con los mismos puntos más un `HookPrior(m10)` entre `clean` y `mid`: el timeline y el undo eligen el previo.
  - *(nuevo)* `timeline_and_undo_pick_the_half_done_merge_photo_for_a_merge_abort`
  - *(nuevo)* `an_unavailable_prior_still_shadows_and_an_older_point_or_none_wins`: un previo no disponible y una observación ensombrecida. El timeline y el undo dan el punto anterior, y sin él, `None` / `TargetUnavailable`.
  - *(nuevo, en `tests/tm_prior_point_undo.rs`)* `raw_git_undo_takes_its_own_guaranteed_prior_before_restoring`: es la premisa de D6. Usa un daemon en proceso como `tests/us_tmc_004.rs`. Tras un `reset --hard` crudo y el undo, el oplog tiene la operación de undo con su `GuaranteedPrior` completo, registrado antes de aplicar. En Windows se ignora como los undos de us_tmc_004 (TQ-14).
- **C7 — la captura registra `mid_operation`.** Una observación a mitad de un rebase con conflicto registra `Some(true)` y una de un worktree limpio, `Some(false)`. Una fila sin el campo se lee como `None`.
  - `an_observation_taken_mid_rebase_records_mid_operation`
  - `an_observation_of_a_clean_worktree_records_clean`
  - `a_complete_row_without_the_field_reads_as_unrecorded`
  - *(nuevo, en `prior_point_tests.rs`)* `mid_operation_is_true_if_in_progress_at_start_or_at_end`: prueba la combinación de D3 como unidad. "En curso solo al final" no es reproducible de forma determinista de extremo a extremo.
- **C8 — merge abortado (enmienda D2 b / D3).** El undo de un `merge --abort` vuelve a la foto del merge a medias, con o sin previos anteriores. El rebase con hook abortado vuelve al previo (`TRADEOFF`). Evidencia de extremo a extremo: `apps/cli/tests/tm_chaos.rs::hostile_half_done_merge_rejects_until_aborted`.
  - `abort_of_a_merge_without_a_prior_picks_the_half_done_merge_photo`: `obs(1,10)`, `mid(2,12)` y eventos `[11, 13]`; el evento 13 elige `mid`.
  - `abort_of_a_merge_after_an_older_prior_and_a_clean_point_picks_the_half_done_merge_photo`: `prior(1,5)`, `obs(2,10)`, `mid(3,12)` y eventos `[6, 11, 13]`; el evento 13 elige `mid`.
  - `abort_of_a_hooked_rebase_after_a_partial_resolution_picks_the_prior`: `prior(1,10)`, `mid(2,11)`, `mid(3,11)` y eventos `[11, 12]`; el evento 12 elige el previo.
  - *(nuevo)* `abort_of_a_hooked_rebase_with_a_late_clean_observation_still_picks_the_prior`: `prior(1,10)`, `obs(2,10)` (limpia pero ensombrecida por (a)), `mid(3,11)` y eventos `[11, 12]`. `obs` no cuenta como `R`, así que el evento 12 elige el previo.

## Convenciones observadas

- `crates/core/src/timemachine/timeline.rs:inferred_tests`: los tests van en un archivo propio, con `#[cfg(test)] #[path = "…_tests.rs"] mod …;`.
- `crates/core/src/timemachine/undo.rs:external_events_in`: es puro sobre lo ya leído del oplog, y el lock solo se toma para leer.
- `crates/core/src/timemachine/oplog/query.rs:snapshot_view`: `complete` sale de `serde_json` sobre el `detail` del journal, y un JSON inválido da `None`.
- `crates/core/src/timemachine/manual.rs:operation_in_progress`: `open_registered_worktree(..).in_progress()`, y si no se puede leer cuenta como en curso.
- `crates/core/tests/tm_store_capture.rs`: usa `tm_common::Env` y repos temporales, nunca este repo.

## Inventario de reutilización

- **Reutiliza:**
  - `crate::observe::open_registered_worktree` + `gitraptor_git::RepoReader::in_progress` para D3.
  - `oplog::{SnapshotView, SnapshotLevel, CompleteInfo}`.
  - `undo::RawSide` y `timeline::EngineSide.raw`/`snapshot_keys` para los seqs.
  - `tm_common::{Env, git, write}` en C7.
- **Genera:** nada.
- **Crea nuevo:** `prior_point.rs` (`PointFacts`, `before_event`). Las búsquedas `code-search "before event prior point"`, `"select snapshot before event"` y `capability-catalog query "prior point" --or "snapshot before event"` solo devuelven las dos copias que se unifican.

## File & Project Topology

- **S1 — tracer bullet** (`criterios: C1 C2 C3 C4 C5`):
  - `crates/core/src/timemachine/prior_point.rs`: D1, D2, D5, con el doc header `TRADEOFF`.
  - `crates/core/src/timemachine/mod.rs`: declara el módulo.
- **S2 — cableado** (`criterios: C6`):
  - `crates/core/src/timemachine/timeline.rs`: `Points::with_events`, se elimina `new` y se adaptan sus tests de `mod tests`. No toca `timeline_prior_point_tests.rs`.
  - `crates/core/src/timemachine/undo.rs`: `raw_candidates`, `raw_target` y su llamada.
- **S3 — condición** (`criterios: C7`):
  - `crates/core/src/timemachine/oplog/model.rs`: el campo nuevo.
  - `crates/core/src/timemachine/store/capture.rs`: escribe `mid_operation`.
  - Los sitios que construyen `CompleteInfo` con literal, si alguno no usa `..Default::default()`.
- **S4 — docs** (sin criterio, D7):
  - `docs/architecture/decisions/ADR-TMC-003-*.md` § 4.
  - `docs/architecture/decisions/ADR-TMC-004-*.md`: tabla de enmiendas.
  - `docs/dev-briefs/us-tmc-005-pre-hook-snapshot.md`: D10 sustituida y nota en D9.

Tests rojos (archivos nuevos, solo del tester):
- `crates/core/src/timemachine/prior_point_tests.rs`
- `crates/core/src/timemachine/timeline_prior_point_tests.rs`
- `crates/core/tests/tm_prior_point_condition.rs`
- `crates/core/tests/tm_prior_point_undo.rs`

## Stubs (los añade el orquestador antes del tester; deben compilar)

1. `prior_point.rs` con `PointFacts` y `before_event`, que devuelve `Vec::new()`. Al final lleva `#[cfg(test)] #[path = "prior_point_tests.rs"] mod tests;`, y `mod.rs` declara `pub(crate) mod prior_point;`.
2. `CompleteInfo.mid_operation: Option<bool>` con el serde de D4, sin escribirlo nunca (queda `None`).
3. `timeline.rs`:
   - `Points::with_events(all, events)`, que llama a `Self::new(all)` e ignora `events`.
   - `#[cfg(test)] #[path = "timeline_prior_point_tests.rs"] mod prior_point_wiring;`.
4. `undo.rs`: `pub(super) fn raw_candidates(..) -> Vec<String>`, que devuelve `Vec::new()`.
5. *(nuevo)* En `prior_point.rs`, `pub(crate) fn mid_operation(at_start: bool, at_end: bool) -> bool`, que devuelve `false`.
6. *(nuevo)* En `timeline.rs`, `Points::offerable(self, points: &[SnapshotView]) -> Self`, que devuelve `self`.

## Supuestos

- Plataforma: los tests unitarios son puros en Linux, macOS y Windows. C7 usa un `git rebase` real con conflicto (argv fijo, repo temporal) y corre en los tres SO. Linux y Windows se validan en CI y en la máquina real, más tarde.
- `settle` (ADR-TMC-004 § 2) espera el cambio del reflog de `HEAD`. Por eso, una observación posterior a un rebase terminado trae marca ≥ evento final. El hueco que queda es el de las refs movidas desde el directorio común sin reflog de `HEAD`, y D2 lo cubre cuando la marca es menor que `f_P`.
- `event_seqs` incluye los eventos de eco: el primer evento tras un previo garantizado es el eco de su operación.
- Una fila `complete` sin el campo (`None`) se trata como limpia: no se puede saber, y tratarla como sucia dejaría el undo de los eventos viejos en `TargetUnavailable`.
- Tests sin `sleep`: el orden de puntos y eventos se inyecta con `PointFacts` y `SnapshotView` armados a mano. El e2e `us_tmc_005_hook` existente queda como evidencia y no se modifica.
- Rendimiento: `before_event` es O(p·log e + p²) en el peor caso por evento. Con p ≤ cientos por worktree no hace falta bench, pero los `f_P` se precalculan una vez por clave en `with_events`.

## Decisiones abiertas

Ninguna. Todo lo de arriba es reversible con un revert y no hay migración.

## Mejoras detectadas

- La pila del undo (`external_events_in`) liga el eco solo por la marca del ancla: un evento persistido después del ancla queda como Git crudo (us_tmc_009). Se arregla en otra rama, con un volcado de CI como prueba (PR #256).
- El motor podría guardar `(pid, start_us)` del `git` atribuido en cada evento. Con eso, D2 sería un enlace exacto por proceso.

## Not Built (deferred)

- **Enlace por proceso `(pid, start_us)`:** cuando el almacén del motor guarde el proceso por evento.
- **Eco tardío del restore en la pila (us_tmc_009):** cuando un volcado de CI confirme la causa.
- **Aviso al restaurar a mano una foto `mid_operation`:** cuando el PO lo pida (US de UX del timeline).
- **Comando sin eventos** (rebase sin cambios, borrado rechazado). Sus observaciones posteriores quedan ensombrecidas hasta el primer evento del worktree, y ese evento elige el previo (más antiguo). `// TRADEOFF: stale-but-recoverable prior for the first event after a no-op hooked command; upgrade with per-event process identity.`
- **Tipo de evento en la regla** (enmienda D2 b), con dos casos:
  - **Rebase sin hook previo:** la foto a mitad sigue siendo elegible para el evento final, como hoy. US-TMC-005 instala el hook que lo cubre.
  - **Rebase con hook, parado en conflicto, resuelto en parte y abortado:** el undo del abort vuelve al previo anterior al rebase, no a las resoluciones parciales. Se recupera: la foto sigue como punto propio y el undo toma su propio previo garantizado.
  - El tipo no lo arreglaría: `rebase (abort)` y `rebase (finish)` son los dos `Rebase` para `watch::repo::kind_of`.
  - Se añadirá cuando el motor distinga fin y abandono de una operación.
  - `// TRADEOFF: a mid-operation photo is excluded only right after a prior; an unhooked rebase may still pick it, and an aborted hooked rebase goes back to its prior; upgrade when engine events tell completion from abort.`
- **`git` ajeno en la misma clave** (riesgo residual de C4): si un `git` que no es el del previo emite un evento en esa clave entre la marca del previo y el primer evento del comando del previo, ese evento fija `f_P` antes de tiempo. Los puntos posteriores a ese evento dejan de estar ensombrecidos y pueden contener ya el efecto del comando del previo. `// TRADEOFF: a foreign git event on the same worktree can close the prior's window before its own command's first event; upgrade with per-event process identity.`
- **`security-expert`:** no aplica. No hay `unsafe`, procesos nuevos ni rutas nuevas; la lectura de `in_progress` ya pasa por la puerta del worktree registrado.
