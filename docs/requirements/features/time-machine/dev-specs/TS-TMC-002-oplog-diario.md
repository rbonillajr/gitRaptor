---
id: DS-TS-TMC-002
title: "Dev Spec — Oplog de la Time Machine con diario de intención y recuperación"
type: dev-spec
status: implemented
feature: time-machine
domain: GRP
story: TS-TMC-002
created: 2026-10-04
updated: 2026-10-08
related:
  adrs: [ADR-TMC-003, ADR-TMC-007, ADR-TMC-001, ADR-GRP-006, ADR-GRP-013, ADR-GRP-005]
  nfrs: [NFR-01, NFR-12, NFR-TMC-06, SEC-TMC-01, SEC-TMC-04, SEC-TMC-09, SEC-06]
tags: [time-machine, oplog, journal, recuperacion, cadena-hash, locks, crates-core]
---

# Dev Spec — TS-TMC-002: oplog, diario y recuperación

Blueprint compacto de [TS-TMC-002](../technical-stories/TS-TMC-002-oplog-diario.md). Fuentes: ADR-TMC-003 (modelo, estados, recuperación), ADR-TMC-007 § 4 (purga en dos fases e interrumpida), ADR-TMC-001 (ubicación `tm/<id-repo>/` y refs `refs/tm/snap/<id>`) y SEC-TMC-01/09. Implementada en esta misma rama.

**Estado `review`**: falta la verificación manual de la TS, revisar con Rene el modelo de estados frente a los escenarios de US-TMC-019. El código no la presupone cerrada.

## 1. Ubicación en el código

Decisión del orquestador (2026-10-04), validada por el Arquitecto: `crates/core::timemachine::oplog`, un módulo nuevo de `core` (TQ-2 → a).

| Archivo | Responsabilidad |
|---|---|
| `crates/core/src/timemachine/oplog/schema.rs` | `OPLOG_MIGRATIONS`: lista propia, versionada con `user_version` y con las mismas reglas que las del perfil (ADR-GRP-006 § 4). No toca `INDEX_MIGRATIONS` ni `STORE_MIGRATIONS` |
| `.../oplog/model.rs` | Registros, estados y tabla de transiciones; `Requester` sin variante "humano" |
| `.../oplog/chain.rs` | Codificación canónica, SHA-256 encadenado, cabeza fuera del oplog y verificación |
| `.../oplog/mod.rs` | `Oplog::open` (carpetas 0700, archivos 0600, cuarentena, verificación y declaración de huecos) y escrituras |
| `.../oplog/query.rs` | Consultas por worktree, periodo, operación, snapshot y nivel; trait `CurrentAttribution`; avisos |
| `.../oplog/stack.rs` | "Última operación" y la pila de undo y redo por ámbito |
| `.../oplog/recovery.rs` | `Oplog::recover`, traits `SnapshotRefs` y `ProcessProbe`, `release_own_lock` |
| `crates/core/src/daemon/mod.rs` | `recover_time_machine` en `Daemon::start`, antes de aceptar operaciones |

Archivo: `<datos>/tm/<repo_id>/oplog.db` y `oplog.head`. El `repo_id` se valida como clave opaca (hexadecimal y guiones) antes de componer la ruta.

## 2. Dependencias nuevas

| Crate | Por qué | Licencia |
|---|---|---|
| `sha2` 0.11 | Hash de la cadena (en el workspace solo estaba SHA-1 de gix) | MIT / Apache-2.0 |
| `serde` / `serde_json` | Columnas estructuradas (ámbito, solicitante, destino, avisos). Ya estaban en `crates/policy` | MIT / Apache-2.0 |

## 3. Esquema

Cinco tablas `STRICT`: `chain`, `snapshots`, `operations`, `journal` y `notices`. Cada fila tipada lleva un `seq` global único, y `chain` guarda por `seq` el tipo, el formato de codificación, el lote, `prev_hash` y `hash`. Los triggers `BEFORE UPDATE/DELETE` hacen `RAISE(ABORT)` en las cinco. La purga no borra filas: anota estados.

| Entidad | Columnas clave | Lo que no guarda |
|---|---|---|
| `snapshots` | id, nivel (`guaranteed-prior`, `observation`, `hook-prior`), worktrees, `store_ref`, marca del motor, operación o evento causante | Tamaño y exclusiones: se conocen al completar y van en el `detail` de la transición `complete` |
| `operations` | id, tipo (`protected` + subtipo, `undo`, `redo`, `restore`), ámbito, **solicitante congelado**, canal, confirmación, destino, avisos, **marca del motor** | El snapshot previo: va en la transición `prior-snapshot` (la fila se escribe en la intención) |
| `journal` | `snapshot-state`, `operation-state` (con paso), `lock-taken` (ruta, inodo), `lock-released`, `child-started`/`child-ended` (pid), `notice-delivered` (canal), `chain-break` (causa, seq) | — |
| `notices` | `interruption` (por worktree) o `purge` (repo), operación, detalle | Su entrega: va en el diario |

## 4. Estados y transiciones

| Operación | Siguientes permitidos |
|---|---|
| `intent` | `prior-snapshot`, `rejected`, `aborted` |
| `prior-snapshot` | `ready`, `rejected` (solape detectado tras el snapshot), `aborted` |
| `ready` | `applying`, `aborted` |
| `applying(n)` | `applying(m > n)`, `finished`, `interrupted` |
| `finished`, `rejected`, `aborted`, `interrupted` | — (terminales) |

| Snapshot | Siguientes permitidos |
|---|---|
| `pending` | `complete`, `discarded` |
| `complete`, `purge-cancelled` | `purge-announced`, `purge-intent` |
| `purge-announced` | `purge-intent`, `purge-cancelled` |
| `purge-intent` | `purged`, `purge-cancelled` |

- `prior-snapshot` exige que el snapshot esté `complete`. Un snapshot se ofrece si su estado es `complete`, `purge-announced` o `purge-cancelled`, su ref existe en el almacén y no está en un hueco de la cadena.
- La fila `pending` se escribe **antes** de crear la ref en el almacén (contrato para TS-TMC-001).

## 5. Cadena de hash (SEC-TMC-09)

- `hash = SHA-256(prev_hash ‖ formato ‖ tipo ‖ seq ‖ lote ‖ columnas de la fila releídas con un SELECT fijo)`, con longitud prefijada. La primera fila cuelga de un génesis derivado del `repo_id`: un oplog copiado de otro repo no verifica.
- Cabeza en `oplog.head` (formato, seq, lote, hash), escrita tras cada commit con archivo temporal 0600, fsync y rename.
- Al abrir se detecta: fila alterada o ausente (`row-altered`), enlace roto (`link-broken`), fila fuera de la cadena (`unchained`), cabeza borrada (`head-missing`), por delante (`head-ahead`, cola cortada), con otro hash (`head-mismatch`) o con filas detrás de más de un lote (`head-behind`). Una cabeza **un solo lote** por detrás (corte entre commit y escritura de la cabeza) se tolera.
- Cada hueco nuevo se declara una sola vez con una entrada `chain-break`. La verificación se re-sincroniza tras cada rotura. Las entidades afectadas salen como `tampered` y no se ofrecen para restaurar.
- Un oplog corrupto va a cuarentena (no se borra) junto con su cabeza, y el nuevo empieza con un hueco `quarantined`.
- **Riesgo aceptado**: el mismo usuario puede recalcular la cadena y la cabeza. Una clave viviría en el mismo perfil, así que no hay HMAC (SEC-TMC-09 es Media).

## 6. Recuperación al arrancar (ADR-TMC-003 § 6, ADR-TMC-007 § 4.5)

`Daemon::start` abre y recupera el oplog de **cada repo observado**, aunque su almacén del motor no abra (Q26). Si un oplog falla, el repo queda en `tm_unavailable` y el daemon arranca igual.

1. Snapshots `pending` → `discarded`. Con almacén, se borran solo las refs de snapshots `pending` o `discarded`. Una ref que el oplog no conoce **se informa y se conserva** (NFR-01). Un `complete` sin ref se informa como hueco.
2. Operaciones en `intent`, `prior-snapshot` o `ready` → `aborted`.
3. `applying` → `interrupted`, con un aviso pendiente por worktree del ámbito (uno de repo si solo toca refs). No se reanuda ni se revierte nada.
4. Purga a medias: con ref → `purge-cancelled`; sin ref → `purged`.
5. **Locks**: se consideran los `lock-taken` sin `lock-released` de operaciones no terminales o interrumpidas. Se borran solo si se cumplen todas estas condiciones:
   - la entrada no está en un hueco;
   - la ruta está dentro del directorio Git común validado;
   - el nombre termina en `.lock`;
   - es un archivo regular con el mismo inodo;
   - ningún hijo anotado sigue vivo.

   El borrado se hace con `statat` y `unlinkat` sobre un descriptor del directorio padre, sin seguir enlaces (SEC-TMC-04). Los hijos vivos se esperan con un tope **compartido por todo el arranque** (`TM_RECOVERY_WAIT` = 5 s); si siguen vivos, el lock se queda y se informa. Es la única escritura en el repo y vive en una sola función (`release_own_lock`).
6. **Sin almacén** (`AbsentStore`, hasta TS-TMC-001): no se decide nada que dependa de refs.

La recuperación es idempotente: un segundo arranque no cambia nada ni genera avisos nuevos.

## 7. Pila de undo y redo (ADR-TMC-003 § 4)

- Función pura sobre la historia del ámbito: un *Do* apila y vacía el redo; un undo saca sus destinos y apila en redo; un redo los devuelve.
- Cuentan como *Do* las operaciones `finished` de tipo `protected` o `restore` y **toda** `interrupted`, sea del tipo que sea: el siguiente undo vuelve a su snapshot previo (US-TMC-019).
- Los eventos de Git crudo del motor los pasa el llamante por secuencia, sin copiarlos. Se descartan los que causó una operación propia y se intercalan por la marca del motor, no por reloj.
- Hay una pila por worktree y otra de **refs del repo**, para las operaciones que no tocan ningún worktree (como en ADR-TMC-007 § 3).

## 8. Decisiones

Decisiones del orquestador (2026-10-04), validadas por el Arquitecto:

| # | Decisión | Ajuste del Arquitecto incorporado |
|---|---|---|
| D1 | Módulo `core::timemachine::oplog`; migraciones propias | Sacarlas de `profile/schema.rs`: el oplog tiene otro dueño. Un oplog en cuarentena empieza con un hueco |
| D2 | Tabla `chain` con SHA-256 | Versión de codificación por fila; génesis ligado al `repo_id` |
| D3 | Cabeza en un archivo aparte | fsync antes del rename; solo se tolera la cabeza un lote por detrás |
| D4 | Tabla de transiciones explícita | `rejected` también desde `prior-snapshot`; `pending` antes de la ref; `purge-cancelled` puede volver a anunciarse |
| D5 | Avisos y su entrega en el diario | Canal cerrado; para la gracia de 24 h solo cuenta la primera entrega en la CLI o la TUI, no el MCP |
| D6 | Recuperación con `SnapshotRefs` | Borrar solo las refs `pending` o `discarded`; `AbsentStore` = "sin almacén"; recuperar aunque el almacén del motor no abra; `ready → aborted` registrado como enmienda de ADR-TMC-003 |
| D7 | Liberación de locks | Las cuatro salvaguardas de § 6.5, tope compartido; compatible con "sin escrituras al repo" como excepción declarada de la TS |
| D8 | Pila por ámbito | `interrupted` de cualquier tipo cuenta; marca del motor en la operación; descartar los eventos propios; pila de refs |
| D9 | `CurrentAttribution` resuelve el actor vigente en cada consulta, sin escribirlo | — |

Decisión propia, sin cambio de ADR: las operaciones `interrupted` siguen siendo candidatas a liberar sus locks en arranques posteriores. Así, un hijo que seguía vivo en un arranque no bloquea Git para siempre, y el inodo sigue protegiendo frente a un lock ajeno.

## 9. Plan de tests (todos en directorios temporales; NFR-01)

| Criterio de la TS | Test |
|---|---|
| Inmutabilidad | `rows_and_journal_reject_update_and_delete`; `invalid_transitions_are_refused` |
| Solicitante congelado | `a_later_correction_does_not_change_the_frozen_requester` |
| Recuperación en cada estado del diario | `recovery_at_every_state_of_the_journal` (corte simulado con todos los estados de operación y de snapshot, refs desconocidas y perdidas, avisos únicos, idempotencia); `without_a_store_nothing_about_refs_is_decided`; `a_clean_stop_leaves_nothing_to_recover`; `an_interrupted_undo_is_the_next_thing_to_undo`; en el daemon, `start_recovers_the_time_machine_oplog` |
| Locks | `an_own_annotated_lock_is_released_and_a_foreign_one_stays`; `a_lock_replaced_since_it_was_annotated_stays`; `a_lock_held_by_a_live_child_waits_and_stays`; `forged_lock_entries_never_delete_anything_else` |
| Pila | `undo_undo_redo_and_a_new_operation_clears_the_redo` y los tests puros de `stack.rs` |
| Aislamiento | `a_corrupt_engine_store_does_not_stop_listing_snapshots`; en el daemon, el almacén del motor con un esquema más nuevo no impide recuperar el oplog |
| Manipulación de la cadena (SEC-TMC-09) | `an_edited_row_is_detected_declared_once_and_not_offered`; `a_removed_row_breaks_the_link`; `head_outside_the_oplog_detects_cut_and_lost_heads`; `a_crash_between_commit_and_head_write_is_not_a_break`; `an_oplog_copied_from_another_repo_does_not_verify`; `a_corrupt_oplog_is_set_aside_and_declared` |
| Otros | Consultas por los cinco criterios; gracia de purga solo con la CLI o la TUI; permisos 0700/0600; claves de repo que escaparían; SQL siempre parametrizado |

## 10. Fuera de alcance (y a quién pertenece)

- Almacén de snapshots y `SnapshotRefs` real: **TS-TMC-001**. El daemon usa `AbsentStore` hasta entonces.
- Aplicador, escrituras en el repo y anotación de locks e hijos en tiempo real: **TS-TMC-003**.
- Resolución del solicitante en el canal: **TS-TMC-004**. Implementación de `CurrentAttribution` sobre el almacén del motor: con la US del timeline (**US-TMC-008**).
- Texto y momento del aviso (US-TMC-019), presentación del timeline (US-TMC-006 a 008), trabajo de purga (US-TMC-016) y qué entra en un undo por agente (US-TMC-011).
- Arnés de caos con muerte real del proceso en cada transición: **INF-TMC-001**. Aquí el corte se simula cerrando el oplog sin terminar y reabriéndolo.

## 11. Pendientes multiplataforma

- ~~Windows no tiene identidad estable de archivo con `std`, así que la recuperación nunca borra un lock allí: lo informa como `unsupported`. Además, `SystemProbe` da por vivo cualquier proceso.~~ Resuelto en XP-15 (2026-10-08) solo en NTFS: ver la Enmienda 2026-10-08.
- **Pendiente: etapa de validación multiplataforma.** Linux: el código es el mismo que en macOS (rustix), pero no se ejecutó.
- Verificado solo en macOS.
- Si un PID se reutiliza, el lock se conserva (es la dirección segura). En Windows ya se compara la hora de inicio del hijo (Enmienda 2026-10-08); en Unix queda para INF-TMC-001.

## Enmienda (2026-10-08, XP-15)

Locks y procesos en Windows. **Decisión del orquestador (2026-10-08), validada por Arquitecto**, que pidió la hora de inicio exacta en lugar de una holgura de reloj y limitar la liberación a NTFS. No cambia el esquema ni el formato de la cadena de hash: `detail` y las columnas ya existían y ya entraban en el hash.

| Cambio | Resolución |
|---|---|
| Identidad del lock en Windows | `inode` = índice de archivo de NTFS (lleva el número de secuencia del registro MFT); `birth_ns` = `CreationTime` (resolución de 100 ns). Se lee por handle, sin seguir enlaces. La hora sola no basta: el *tunneling* de NTFS da a un archivo recién creado la hora de creación del que se borró con el mismo nombre |
| Columna `inode` | u64 guardado bit a bit en el `INTEGER` con signo (complemento a dos). Las filas antiguas eran positivas y se leen igual. En Unix, un inodo mayor que `i64::MAX` ya no hace fallar la anotación |
| Liberación en Windows | Se abre la ruta del lock sin seguir reparse points, con `DELETE` y compartiendo todo. Si es un archivo regular en NTFS con el índice y la hora anotados, se borra por ese mismo handle (`FileDispositionInfo`). Si otro proceso lo tiene abierto con `FILE_SHARE_DELETE`, el nombre queda pendiente de borrado hasta que lo cierre: `released` es optimista en ese caso |
| `unsupported` | Cubre también "el sistema de archivos no es NTFS" (FAT y exFAT reutilizan el índice; en ReFS no es único) |
| `child-started.detail` | `{"start_us": N}`: hora de inicio del hijo en µs desde la época, la misma que lee el canal (`ProcInfo::start_us`) al marcarlo |
| Contrato de `ProcessProbe` | Nuevo método `is_same(pid, start_us)`, que por defecto llama a `is_alive(pid)`. `SystemProbe` en Windows: vivo solo si existe un proceso con ese PID y esa hora exacta; `Gone` u otra hora (PID reutilizado), muerto; acceso denegado o lista ilegible, vivo (fail-closed). Sin `start_us` (filas antiguas), como `is_alive`. En Unix sigue `kill(0)` |

Tests: sección "Locks y procesos en Windows (XP-15)" de [xplat-pendientes.md](../../../../architecture/xplat-pendientes.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #32, #176.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- Revisión manual con Rene del modelo de estados frente a US-TMC-019 (la DS sigue en `review`).
- PID reutilizado en Unix: INF-TMC-001.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
