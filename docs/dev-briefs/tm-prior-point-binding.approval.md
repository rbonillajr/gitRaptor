---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/tm-prior-point-binding.md"
---

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas o se estancó; ver «Diferidos»
- Generada: `2026-10-21T17:07:26.214Z`
- Sha de aprobación: `762b2790fab67d0aaa03dadbddc6b12db73f8424cd0e7206d67c49ebbb181445:63e4a00d38045de8c432ed5e7121fde1738824774602e54443eddfbfdc2bc47a:4d088b7f3b0c0d90438ab171f05c5bae49167e13afe0d1399b3a86f1b36a49ad`

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES f61b634a40af>>>
```text
docs/dev-briefs/tm-prior-point-binding.md
```
<<<FIN-DATOS-NO-CONFIABLES f61b634a40af>>>

planSha: `762b2790fab67d0aaa03dadbddc6b12db73f8424cd0e7206d67c49ebbb181445` (base de la cadena de re-planes)

Contenido:

<<<DATOS-NO-CONFIABLES 676c9284678b>>>
```text
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
  - **Ventana:** para cada previo `P` (`HookPrior` o `GuaranteedPrior`) de la clave `k`, `f_P` es el primer evento crudo de `k` con `seq > P.mark` (∞ si no hay).
  - **Sombra:** un punto `Q` de `k` queda **ensombrecido por `P`** si `Q.seq > P.seq` y `Q.mark < f_P`. Se registró después del previo, pero antes de que el motor persistiera el primer evento de ese comando, así que su foto puede contener ya el efecto del comando con una marca atrasada. Un punto ensombrecido nunca es "antes de un evento".
  - **Alcance:** se aplica a todos los niveles, también a un previo posterior, y por clave: los eventos de otro worktree no mueven `f_P`.
  - **`floor` (ajuste 2):** decide quién puede ganar, no quién proyecta sombra. Un previo de la generación actual proyecta sombra aunque no sea disponible.
  - **Desviación consciente del ajuste 2:** un previo con `seq < floor` no ensombrece puntos con `seq >= floor`.
    - Sus marcas son de otra numeración (ADR-TMC-003 § 4, "Generación"), y `f_P` contra eventos de la generación nueva sería ∞.
    - Así ensombrecería todo lo posterior, y cada cambio de generación dejaría el undo en `TargetUnavailable`.
    - Entre puntos bajo `floor` sí ensombrece, aunque ninguno de ellos puede ganar.
- **D3. Observación a mitad de operación.** Toda captura completa guarda `mid_operation = en curso al inicio || en curso al final` (`RepoReader::in_progress` en alguno de sus worktrees, antes de leer y otra vez en el punto de validez).
  - **Exclusión:** una observación o un punto manual con `Some(true)` nunca es "antes de un evento".
  - **Previos:** un previo nunca se excluye por `mid_operation` (D9 de US-TMC-005 sigue vigente). Lo toma con `rebase-merge/` ya presente (`--autostash`) y es la foto anterior por construcción.
  - Las fotos siguen en el timeline como puntos propios.
  - **Combinación:** la hace `prior_point::mid_operation(at_start, at_end)`.
  - **Qué se prueba:** una operación que empieza durante la captura ya la descarta casi siempre la guarda `GitState` (HEAD o índice cambian). Por eso, el caso "en curso solo al final" se prueba sobre la función, no de extremo a extremo.
- **D4. Sin migración.** `mid_operation` va en `CompleteInfo` (JSON del `detail` de la entrada `complete` del journal, que ya está encadenada). Usa `#[serde(default, skip_serializing_if = "Option::is_none")]` y es `Option<bool>`. `None` (filas anteriores al fix) cuenta como "no a mitad de operación", que es el comportamiento de hoy. No hay `OPLOG_MIGRATIONS` nuevo y los hashes no cambian.
- **D5. Orden.** Entre los elegibles (`mark < seq`, `seq >= floor`, sin sombra y sin `mid_operation`), gana el mayor `(mark, seq)`. Con la misma marca, un previo registrado después de una observación gana por `seq`: es el caso de D10, que sigue cubierto.
- **D6. Fallar cerrado = más antiguo, nunca a mitad.** Si hay duda, se elige un punto anterior. Restaurar de más siempre se recupera (el undo toma su propio previo garantizado, NFR-01), mientras que una foto a mitad de operación o posterior al evento pierde el estado previo. Ver `TRADEOFF` en Not Built.
- **D7. Documentación.**
  - **ADR-TMC-003 § 4:** se enmienda la fila "Git crudo en la pila" con el destino de D1, D2, D3 y D5. Se documenta `mid_operation` como parte del esquema del `detail` de la entrada `complete`. Se registra como límite abierto de NFR-01 el bug de pila de us_tmc_009 (`external_events_in` liga el eco solo por la marca del ancla; PR #256).
  - **ADR-TMC-004:** se añade una fila a su tabla de enmiendas: la captura registra `mid_operation`, y una observación a mitad de operación ya no es un punto "antes de un evento".
  - **`us-tmc-005-pre-hook-snapshot.md`:** D10 se marca **sustituida** por `tm-prior-point-binding` D1/D5. Se anota en D9 que sigue vigente: un previo nunca se descarta por `mid_operation`.

**Cierra el bug de us_tmc_009 (`a_restore_can_be_undone_back_to_the_exact_prior_state`)? No.** Ese fallo está en la **pila**: el evento tardío 3 cae fuera del eco del restore (`external_events_in`, el ancla con una marca anterior a la del evento), y `last_operation` es `GitEvent(3)`. Este fix no cambia el eco. Solo arregla el **estado** al que lleva ese undo: con D2, el ancla queda ensombrecida por el previo garantizado del restore, y deshacer el evento 3 restaura el estado exacto anterior al restore, no el ancla. La aserción `undone_operation_id == restore.operation_id` puede seguir fallando.

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
  - `rebase_without_a_prior_skips_the_mid_operation_observation`
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
- **C6 — timeline y undo eligen lo mismo**, y el undo nunca apunta a una observación a mitad de operación.
  - `timeline_and_undo_pick_the_same_point_for_an_event`
  - `undo_never_targets_a_mid_operation_observation`
  - *(nuevo)* `an_unavailable_prior_still_shadows_and_an_older_point_or_none_wins`: un previo no disponible y una observación ensombrecida. El timeline y el undo dan el punto anterior, y sin él, `None` / `TargetUnavailable`.
  - *(nuevo, en `tests/tm_prior_point_undo.rs`)* `raw_git_undo_takes_its_own_guaranteed_prior_before_restoring`: es la premisa de D6. Usa un daemon en proceso como `tests/us_tmc_004.rs`. Tras un `reset --hard` crudo y el undo, el oplog tiene la operación de undo con su `GuaranteedPrior` completo, registrado antes de aplicar. En Windows se ignora como los undos de us_tmc_004 (TQ-14).
- **C7 — la captura registra `mid_operation`.** Una observación a mitad de un rebase con conflicto registra `Some(true)` y una de un worktree limpio, `Some(false)`. Una fila sin el campo se lee como `None`.
  - `an_observation_taken_mid_rebase_records_mid_operation`
  - `an_observation_of_a_clean_worktree_records_clean`
  - `a_complete_row_without_the_field_reads_as_unrecorded`
  - *(nuevo, en `prior_point_tests.rs`)* `mid_operation_is_true_if_in_progress_at_start_or_at_end`: prueba la combinación de D3 como unidad. "En curso solo al final" no es reproducible de forma determinista de extremo a extremo.

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
- **`git` ajeno en la misma clave** (riesgo residual de C4): si un `git` que no es el del previo emite un evento en esa clave entre la marca del previo y el primer evento del comando del previo, ese evento fija `f_P` antes de tiempo. Los puntos posteriores a ese evento dejan de estar ensombrecidos y pueden contener ya el efecto del comando del previo. `// TRADEOFF: a foreign git event on the same worktree can close the prior's window before its own command's first event; upgrade with per-event process identity.`
- **`security-expert`:** no aplica. No hay `unsafe`, procesos nuevos ni rutas nuevas; la lectura de `in_progress` ya pasa por la puerta del worktree registrado.
```
<<<FIN-DATOS-NO-CONFIABLES 676c9284678b>>>

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 8dea0918b275>>>
```text
docs/dev-briefs/tm-prior-point-binding.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 8dea0918b275>>>

contractSha: `63e4a00d38045de8c432ed5e7121fde1738824774602e54443eddfbfdc2bc47a`

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES a88f34dc1faf>>>
```text
[1] id: C1
statement: Rebase con observación tardía: con HookPrior(m0), Obs(m0) anterior, evento 1, Obs(m1) a mitad de operación y evento 2, el punto previo de los eventos 1 y 2 es el previo del hook; sin previo es Obs(m0), nunca la foto a mitad de rebase
verify: cargo test -p gitraptor-core --lib timemachine::prior_point::tests::rebase_ 2>&1 | grep -q 'test result: ok. 2 passed'
tests:
  - crates/core/src/timemachine/prior_point_tests.rs

[2] id: C2
statement: Restore con evento Git tardío: con GuaranteedPrior(m2) y un ancla Obs(m2) registrada después, el punto previo del evento 3 es el previo garantizado, no el ancla; con el ancla en m3, el evento 4 elige el ancla
verify: cargo test -p gitraptor-core --lib timemachine::prior_point::tests::restore_ 2>&1 | grep -q 'test result: ok. 2 passed'
tests:
  - crates/core/src/timemachine/prior_point_tests.rs

[3] id: C3
statement: Borrado de rama con previo: una observación registrada después del previo y antes de que el motor persista el BranchDelete, con la misma marca o con la marca subida por otro worktree, nunca gana al previo para ese evento
verify: cargo test -p gitraptor-core --lib timemachine::prior_point::tests::branch_deletion_ 2>&1 | grep -q 'test result: ok. 2 passed'
tests:
  - crates/core/src/timemachine/prior_point_tests.rs

[4] id: C4
statement: Operaciones de dos worktrees intercaladas: cada previo se liga solo a los eventos de su propio worktree y los eventos del otro no cierran su ventana
verify: cargo test -p gitraptor-core --lib timemachine::prior_point::tests::interleaved_ 2>&1 | grep -q 'test result: ok. 2 passed'
tests:
  - crates/core/src/timemachine/prior_point_tests.rs

[5] id: C5
statement: Controles negativos: un punto limpio registrado con marca igual o posterior al primer evento del comando es el previo del evento siguiente, y nunca se elige un punto bajo el suelo de generación ni con marca igual o posterior al evento; un previo bajo el suelo no ensombrece puntos de la generación actual
verify: cargo test -p gitraptor-core --lib timemachine::prior_point::tests::control_ 2>&1 | grep -q 'test result: ok. 3 passed'
tests:
  - crates/core/src/timemachine/prior_point_tests.rs

[6] id: C6
statement: El timeline (Points::with_events) y el undo (raw_candidates) eligen el mismo punto previo para el mismo evento con una sola función y sobre el mismo conjunto sin filtrar (sombra con todos los puntos, disponibilidad filtrada sobre el resultado); un previo no disponible sigue ensombreciendo y gana un punto anterior o ninguno (TargetUnavailable); el undo nunca apunta a una observación a mitad de operación; y el undo de Git crudo toma su propio previo garantizado antes de restaurar
verify: cargo test -p gitraptor-core --lib timemachine::timeline::prior_point_wiring:: 2>&1 | grep -q 'test result: ok. 3 passed' && cargo test -p gitraptor-core --test tm_prior_point_undo -- --exact raw_git_undo_takes_its_own_guaranteed_prior_before_restoring 2>&1 | grep -q 'test result: ok. 1 passed'
tests:
  - crates/core/src/timemachine/timeline_prior_point_tests.rs
  - crates/core/tests/tm_prior_point_undo.rs

[7] id: C7
statement: Una observación tomada a mitad de un rebase con conflicto registra mid_operation Some(true) en su CompleteInfo, una de un worktree limpio Some(false), y una fila complete sin el campo se lee como None, en repos temporales; mid_operation es verdadero si hay una operación en curso al inicio o al final de la captura (función combinadora probada como unidad)
verify: cargo test -p gitraptor-core --test tm_prior_point_condition 2>&1 | grep -q 'test result: ok. 3 passed' && cargo test -p gitraptor-core --lib timemachine::prior_point::tests::mid_operation_ 2>&1 | grep -q 'test result: ok. 1 passed'
tests:
  - crates/core/tests/tm_prior_point_condition.rs
  - crates/core/src/timemachine/prior_point_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES a88f34dc1faf>>>

## Tests de criterio

testsSha: `4d088b7f3b0c0d90438ab171f05c5bae49167e13afe0d1399b3a86f1b36a49ad`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES a6b9d043e771>>>
```text
e63f8abf3c55c0ac1d691e12b7cc534749e8a99255d272fa2f1952359ceb01f6  crates/core/src/timemachine/prior_point_tests.rs
2ffd1eff34ec91ca6612412b3c0153124288e41b871536335f63ba28f1eeca0c  crates/core/src/timemachine/timeline_prior_point_tests.rs
21ff50b06703862af3679332be7074ca33001bfce864d00b499cd014d227c4b0  crates/core/tests/tm_prior_point_condition.rs
39e0d51f3ad40ce1b162934f381c2629e6f890231d1443f19e14ad51f0abbe5d  crates/core/tests/tm_prior_point_undo.rs
```
<<<FIN-DATOS-NO-CONFIABLES a6b9d043e771>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 1 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES a09e66e29fc5>>>
```text
id: PC-2
kind: contradiccion
toHuman: false
evidence: docs/dev-briefs/tm-prior-point-binding.md:112
ask: El combinador `prior_point::mid_operation` (stub 5, D3) vive en `prior_point.rs`, que es de S1 (`criterios: C1 C2 C3 C4 C5`, 'D1, D2, D5'). Pero su test `mid_operation_is_true_if_in_progress_at_start_or_at_end` está en el verify de C7, y C7 es de S3, que no lista `prior_point.rs`. Así, S3 tiene que editar un archivo de S1 o C7 queda sin dueño. Asigna explícitamente `mid_operation` (D3) a S1 y añade C7 a sus criterios, o pasa la función a un archivo de S3, para que cada archivo pertenezca a una sola slice.
```
<<<FIN-DATOS-NO-CONFIABLES a09e66e29fc5>>>

## Firmas

- planSha (contexto, no firmado): `762b2790fab67d0aaa03dadbddc6b12db73f8424cd0e7206d67c49ebbb181445`
- contractSha: `63e4a00d38045de8c432ed5e7121fde1738824774602e54443eddfbfdc2bc47a`
- testsSha: `4d088b7f3b0c0d90438ab171f05c5bae49167e13afe0d1399b3a86f1b36a49ad`
- approvalSha: `762b2790fab67d0aaa03dadbddc6b12db73f8424cd0e7206d67c49ebbb181445:63e4a00d38045de8c432ed5e7121fde1738824774602e54443eddfbfdc2bc47a:4d088b7f3b0c0d90438ab171f05c5bae49167e13afe0d1399b3a86f1b36a49ad`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
