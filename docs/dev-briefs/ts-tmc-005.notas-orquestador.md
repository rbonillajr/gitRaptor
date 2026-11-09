# Notas del orquestador para el diseño de TS-TMC-005 (insumo, no plan)

Fuente: `docs/requirements/features/time-machine/technical-stories/TS-TMC-005-undo-espera-punto-base.md` y ADR-TMC-005 § Nota (2026-11-08). La TS dice que no lleva Dev Spec propia: el plan es el Brief de /implement.

## Lo leído en el código

- `crates/core/src/timemachine/continuous.rs`: hilo `run` con `pending: HashMap<(repo, worktree), Pending>`; `Pending.event` = máx. seq del evento sin capturar; se quita de `pending` en `Attempt::Done` (éxito o fallo definitivo), al agotar `MAX_RETRIES` y con `Signal::Forget`. `ContinuousCapture::git_event` lo llama `daemon/modules/tm_capture.rs` desde `daemon/repos.rs:659`, después de `marks.set_persisted` (repos.rs:567): hay una ventana mínima en la que el evento ya está persistido y el undo lo ve, pero la señal aún no llegó.
- `CaptureDeps` (continuous.rs) se construye en `daemon/mod.rs::capture_deps()` (cada vez nuevo) y en ~20 tests. `UndoEnv.engine: Option<&CaptureDeps>`.
- `overlap.rs::base_point_of` elige B; `agent_base` (para el undo por agente) lo envuelve. `undone_bases` → `check_with`.
- `undo.rs::undo_selected`: toma el `repo_lock` (lock_queued) primero, luego `settle`, raw events, `overlap_started`, y `plan()` (undo simple, `--since`) o `undo_agent::plan()`. `plan()` suelta el lock del oplog antes de la fase 1 (lee ops/snaps y lo suelta). El presupuesto de la fase 2 es `OVERLAP_BUDGET - overlap_started.elapsed()`.
- `undo_agent::plan`: calcula el conjunto con el oplog tomado, lee `snaps`, suelta el lock y luego `Between::change` → `agent_base` → sin B: `OverlapUnverified` (undo_agent.rs:~518).
- La captura continua NO toma `repo_lock` (solo el lock del oplog y el writer del almacén en `store.capture`). Esperar con el `repo_lock` tomado no interbloquea.
- El redo usa como B el ancla del undo (`OpRef::Oplog`), que se toma síncrona tras la operación: nunca espera a la captura continua.

## Propuesta (a validar por el diseño)

- Nuevo `crates/core/src/timemachine/base_wait.rs`: `CaptureSeals` (Mutex + Condvar) con, por (repo, worktree), `signalled` (máx. seq señalado) y `settled` (máx. seq cuya captura terminó: sellada o abandonada), y `running` (captura activa y `enabled`). Va en `CaptureDeps` como `seals: Arc<CaptureSeals>` (en tests `Default` = no corre → no espera, comportamiento de hoy). El daemon crea uno y lo comparte.
- `ContinuousCapture::git_event` marca `signalled` síncrono antes de enviar; el hilo marca `settled` y `notify_all` al quitar el pendiente (Done, agotado, Forget); `start` pone `running`, `stop`/`Drop` lo quita y notifica.
- Espera: `wait_for(repo, worktree, seq, deadline)` con `Condvar::wait_timeout`, sin `sleep`. Pendiente = running && settled < seq. Si aún no está señalado y `seq` > máx. señalado del repo: espera breve de traspaso (≤ 100 ms, por condvar) a que llegue la señal; si no, `not-pending`.
- Un solo camino: helper que recibe los `OpRef::GitEvent` del conjunto sin B (referencia agente), espera, relee snapshots y reaplica `base_point_of`. Lo llaman `undo::plan` (antes de la fase 1) y `undo_agent::plan` (antes de `Between::change`). El plazo de las dos fases se corre lo esperado (`Planned.waited`).
- Log `tm_base_wait` con `base_wait_ms` y `result` (`found`/`expired`/`not-pending`/`abandoned`), solo id de repo (SEC-12).
- `BASE_WAIT` = 2 s como supuesto; medir antes de fijar (la tarea lo pide) y dejar la cifra en el PR.
- Desviación de la TS a validar: la TS dice «antes de tomar el cerrojo de escritura del repo»; el código actual planifica con el `repo_lock` tomado y la captura no lo usa, así que se espera con él tomado (sin oplog ni writer). Registrar como decisión validada por el Arquitecto.
- Tests: CA-1 a CA-7 de la TS. CA-1 e2e en `apps/cli/tests/raw_git_undo.rs` y `undo_overlap_same_session` (quitar la espera previa al undo). Retener la captura con `CaptureLayer` que bloquea (esperando un canal con plazo) y, dentro, comprueba que el lock del oplog está libre (CA-5).

## Coordinación

TD-TMC-003 (rama `fix/TD-TMC-003-agent-overlap-echoes`, worktree `../tdtmc003`) toca `undo_agent.rs`, `check_selected_tail` y `branch_mover`. No subir hasta que esté en `main`; entonces rebasar.
