---
id: DS-US-TMC-004
title: "Dev Spec — Captura continua de lo hecho con Git crudo y en el editor"
type: dev-spec
status: approved
feature: time-machine
domain: GRP
story: US-TMC-004
created: 2026-10-06
updated: 2026-10-06
related:
  stories: [US-TMC-004, US-TMC-001, US-TMC-002, US-GRP-002, US-GRP-004, US-GRP-007, US-GRP-009]
  enablers: [TS-TMC-001, TS-TMC-002, TS-TMC-003, TS-TMC-004, SPIKE-TMC-001]
  adrs: [ADR-TMC-001, ADR-TMC-003, ADR-TMC-004, ADR-TMC-006, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013]
  rules: [BR-TMC-CONS-002, BR-TMC-CONS-003, BR-TMC-WF-001]
  nfrs: [NFR-01, NFR-04, SEC-TMC-12]
tags: [time-machine, captura-continua, git-crudo, observacion, undo, m1, criterio-3]
---

# Dev Spec — US-TMC-004: captura continua de lo hecho con Git crudo

Plano compacto de [US-TMC-004](../user-stories/US-TMC-004-captura-continua-git-crudo.md). Une lo que ya está en `main`: la captura del almacén con nivel `observation`, el tope de 50 MB y las credenciales (TS-TMC-001), el oplog y su pila con eventos externos (TS-TMC-002), el aplicador (TS-TMC-003), la operación protegida (TS-TMC-004), `raptor undo` (US-TMC-002), el observador en vivo (US-GRP-002) y la atribución de sesiones (US-GRP-007/009).

**Qué entrega**: dentro del daemon, la Time Machine captura cada worktree observado a partir de lo que el motor ya observó (ADR-TMC-004 § 2), y el Git crudo que el motor registra entra en la pila de `raptor undo` con su actor. Es el criterio de salida 3 de M1: un agente que hace `git reset --hard` con trabajo sin commitear lo recupera con `raptor undo` desde su shell.

**Qué no entrega**: el snapshot previo vía hook (US-TMC-005), el timeline (US-TMC-006/007/008), la cuota y el hueco "sin espacio" del almacén (US-TMC-022: aquí un `ENOSPC` es una captura fallida), y la interfaz de rutas con continuidad del motor (escalón 2, § 6). El overhead del snapshot previo sigue en US-TMC-020; el presupuesto del motor con la captura activa se verifica aquí (§ 5).

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `crates/api/src/messages.rs` | `GitEventKind::Reset`: un `reset` que no mueve la rama (`HEAD` en el mismo commit) o con `HEAD` separado |
| `crates/core/src/watch/repo.rs` | El repo lee el tramo nuevo del reflog de `HEAD` de cada worktree y publica `reset` (§ 2.1) |
| `crates/core/src/watch/{mod,repo,worktree}.rs` | `ObserverHooks::worktree_changed` (el worktree avisa al cerrar una ventana con rutas no ignoradas o con estado distinto); cada lote lleva el tamaño del reflog de `HEAD` (repo) o el `HEAD` (worktree) que leyó; `FanoutHooks` |
| `crates/core/src/timemachine/engine.rs` (nuevo) | `EngineLink` (marca, calma por estado, eventos de Git crudo de un worktree, generación), `RepoMarks`, `GitState` (guarda) y `generation_floor` |
| `crates/core/src/profile/store.rs`, `crates/core/src/timemachine/oplog/{mod,stack}.rs` | `RepoStore::last_seq` y `generation` (en `store_meta`); `Oplog::last_seq`; `undo_stack_in` con las filas de otra generación delante de todo evento |
| `crates/core/src/executor/mod.rs` | `RunEnv.engine_mark` (marca en calma o `git-busy`) y `RunEnv.after_step` (ancla) |
| `crates/core/src/channel/bus.rs`, `crates/api/src/lib.rs` | Protocolo 7: un cliente anterior no recibe eventos `reset` (ni por suscripción ni en `events.history`) |
| `crates/core/src/timemachine/continuous.rs` (nuevo) | El servicio de captura continua: disparadores, coalescencia, guarda de consistencia y anclas posteriores |
| `crates/core/src/timemachine/store/capture.rs` | `CaptureRequest.still_valid`: la captura se descarta antes del punto de validez si deja de ser consistente (`CaptureError::Discarded`) |
| `crates/core/src/timemachine/undo.rs` | El Git crudo entra en la pila; destino, actor y ámbito de un `GitEvent` |
| `crates/core/src/daemon/{mod,shutdown,tm,env}.rs` | Cableado: `RepoMarks` (marca y lo que el motor leyó, después de persistir cada lote), el servicio, `Control::RawEvents`, `DaemonConfig.tm_capture` y `GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR` (solo debug) |
| `crates/core/src/channel/{mod,conn}.rs` | `engine_mark` de operaciones y undos con la secuencia del repo; ancla posterior tras una operación protegida o un undo |
| `apps/cli/src/events.rs`, `apps/cli/i18n/{en,es}.txt` | Texto en/es del evento `reset` |

## 2. Diseño

### 2.1 El `reset` que no mueve la rama es un evento del motor

`git reset --hard` sobre el mismo commit (el caso típico de un agente que descarta trabajo) no mueve ninguna rama: hoy el motor no publica nada. Git sí añade una entrada al reflog de `HEAD` del worktree (`reset: moving to HEAD`). El repo guarda, por worktree, el tamaño de `logs/HEAD` (solo metadatos); cuando crece, lee **solo el tramo nuevo** (como mucho 64 KiB, sin seguir enlaces) y publica `reset` por cada entrada `reset:` con el commit igual antes y después, o con `HEAD` separado. Los `reset` que mueven una rama siguen siendo `branch-update` (ya los nombra el reflog de la rama), así que nada cuenta dos veces. Si el archivo se acorta (`reflog expire`), solo se toma el tamaño nuevo.

### 2.2 Marca del motor común y generación (ADR-GRP-013, ADR-TMC-003 § 1)

La pila de undo intercala operaciones y eventos por la marca del motor. Hasta ahora las operaciones guardaban la secuencia del bus (por ejecución) y los eventos la del repo: dos numeraciones distintas. Desde esta historia, **toda marca es la última secuencia de evento persistida del repo** (`RepoMarks`, la mantiene el bucle del daemon después de cada lote escrito; al arrancar sale del almacén). La usan la intención de una operación, el undo y cada captura.

Las marcas solo se comparan dentro de una **generación** del almacén del motor: si se pierde y se recrea (Q26), su secuencia vuelve a 1. El almacén guarda un id de generación en `store_meta` y la Time Machine anota en `tm/<repo>/engine-generation` desde qué fila del oplog vale. Las filas anteriores (otra generación, o la secuencia del bus anterior a esta historia) van antes de todo evento en la pila y nunca son destino de un evento.

### 2.3 Calma del motor, por estado

El motor persiste un evento algo después del `git` que lo causa. `EngineLink::settle(repo, worktrees, tope)` espera a que **cada worktree esté en calma**: el tamaño de su reflog de `HEAD` y su `HEAD` en disco son los que el motor leyó en su último lote persistido (el repo lee el reflog y nombra los eventos de ramas y `reset`; el worktree lee `HEAD` y nombra el cambio de rama), y ningún `git` tiene su `index.lock`. Todo `git` que cambia el estado de un worktree escribe ahí (commit, checkout, merge, rebase, reset, stash), así que en calma la marca cubre todo `git` ya terminado, aunque su notificación del sistema llegue tarde. Sin reflog, el respaldo es por tiempo: 150 ms sin cambios en el directorio Git. Las ediciones de archivos no impiden la calma. Con el tope agotado devuelve `None`: una captura se reprograma; un undo se rechaza con `repo-busy` y una operación del catálogo con `git-busy` (nunca se planifica con una marca vieja).

### 2.4 Servicio de captura continua (ADR-TMC-004 § 2)

Un hilo por daemon (`raptor-tm-capture`) recibe del motor, sin bloquearlo nunca (canal sin límite que se drena; el motor no espera):

- **Actividad** de un worktree (`worktree_changed`, fuera del presupuesto de 300 ms: después de entregar el lote).
- **Eventos de Git** persistidos, con su secuencia (desde `observed`, después de publicar).

Por worktree: captura cuando lleva `Q` = 1 s sin actividad, o como mucho cada `M` = 5 s con actividad continua; e **inmediata** tras un evento de Git, con `cause_event_seq`. Coalescencia: una captura en curso a la vez y, por worktree, solo la petición más reciente. Cada captura:

1. Espera la calma (§ 2.3) con un tope de 1 s; sin calma, se reprograma.
2. Por debajo del suelo de espacio libre de SEC-TMC-12, máx(5 GB, 5 %), la captura se omite y se registra (`tm_capture_failed`, `no-space`).
3. Pide al almacén una captura `observation` del worktree (clave estable, opción de credenciales del perfil leída en cada captura) con la marca de la calma.
4. **Guarda de consistencia** (`still_valid`), evaluada justo antes del punto de validez: el reflog de `HEAD`, `HEAD`, la identidad del índice y `index.lock` del worktree son los del paso 1. Si cambiaron, la captura se descarta sin crear punto (`Discarded`) y se repite. Así una captura nunca mezcla el estado de antes y el de después de un `git`, y nunca lleva una marca anterior a un evento cuyo efecto ya leyó.
5. Resultado: `complete` es el punto; `Yielded`/`Discarded`, se repite; cualquier otro error, la captura no crea punto (fila `discarded` si llegó a `pending`) y se registra `tm_capture_failed` con su tipo (escenario 5). La siguiente actividad vuelve a intentarlo.

Lo grande (> 50 MB) queda fuera y la captura es parcial (ya en TS-TMC-001); ignorados y credenciales, igual que en US-TMC-001.

### 2.5 Ancla posterior y `caused_by`

Las escrituras de una operación de GitRaptor (undo incluido) también producen eventos en el motor; si entraran como Git crudo, el siguiente `raptor undo` desharía el efecto del undo en lugar de retroceder (nota de la Dev Spec de US-TMC-002). Al terminar una operación protegida o un undo, con el repo aún tomado, el daemon espera la calma (tope 2 s) y toma un **ancla**: una captura `observation` de los worktrees del ámbito con `cause_operation` = la operación. Un evento de un worktree del ámbito con `marca de la operación < seq ≤ marca del ancla` tiene `caused_by` = esa operación y no entra en la pila. El ancla es además el estado posterior a la operación: el destino natural para deshacer el siguiente Git crudo. Si el ancla falla, la operación sigue terminada y los eventos hasta la siguiente captura del worktree se toman como causados (lo seguro: un undo no confunde el eco con trabajo del agente). La marca de intención de la operación también es en calma (§ 2.3), así que un `git` crudo justo antes nunca cae en su eco.

### 2.6 Git crudo en la pila de undo

- **Calma antes del plan**: con el repo tomado, el undo espera la calma (tope 2 s), para que un `git` recién terminado esté en la pila; sin calma, `repo-busy`.
- **Eventos que cuentan** (ADR-TMC-003 § 4): `commit`, `merge`, `rebase`, `branch-update`, `branch-switch` y `reset` del worktree. Si el más reciente es de otro tipo (`push`, `reconciled`, crear o borrar ramas y worktrees; deshacerlos es US-TMC-009/014), el undo responde `raw-git-not-covered` y no lo salta.
- **Destino** de `GitEvent(seq)`: la última captura disponible, verificada y sin manipular que incluye el worktree, de la generación actual, con `marca < seq` (de cualquier nivel). Sin ella: `rechazada` con `target-unavailable` (un punto que no existe nunca se presenta como protegido).
- **Actor**: la sesión a la que el motor atribuyó el evento (`Requester::Agent` con su id de sesión) o "sin atribuir". La regla base de permisos de US-TMC-002 no cambia: el agente deshace lo suyo; "sin atribuir" deshace lo no atribuido.
- **Ámbito**: el worktree del evento y la rama que nombra; el plan añade las ramas de `HEAD` (D2 de US-TMC-002). En `UndoResult`, `undone_operation_id` = `git-event-<seq>` y `undone_subtype` = el tipo del evento.

### 2.7 Contrato y pruebas

- **Protocolo 7** (`API_VERSION` 7.0.0): el tipo `reset` es nuevo. Un cliente de protocolo 5 o 6 no lo sabría leer, así que no lo recibe ni por suscripción ni en `events.history`.
- `DaemonConfig.tm_capture` fija `Q`, `M`, una capa de fallos para los tests y si se omite el suelo de espacio libre; capa y suelo solo cuentan en builds con `debug_assertions` (mismo patrón que `tm_prior_layer`). El binario de debug lee `GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR=1` para el e2e. Los tests usan `Q` corto, nunca esperas fijas.

## 3. Decisiones

Todas son **Decisión del orquestador (2026-10-06), validada por el Arquitecto** (ver § 7):

| # | Decisión | Motivo |
|---|---|---|
| D1 | `GitEventKind::Reset` para el `reset` que no mueve rama, leído del tramo nuevo del reflog de `HEAD` | Sin él, el `reset --hard` del criterio 3 no es una operación del timeline (ADR-TMC-003 § 4) y no hay nada que deshacer |
| D2 | Marca común = última secuencia persistida del repo | Las operaciones usaban la secuencia del bus y los eventos la del almacén: el orden de la pila no era comparable |
| D3 | Calma **por estado** (reflog de `HEAD` y `HEAD` en disco = los del último lote persistido, sin `index.lock`) antes de capturar, de la intención de una operación o un undo y del ancla; por tiempo solo sin reflog | El motor persiste un evento después del `git`. Sin calma, una captura posterior al `reset` podía llevar una marca anterior y ser el destino del undo: se perdería el trabajo que se quería recuperar (NFR-01). Por tiempo dependía de la latencia de las notificaciones (bloqueante B1 del Arquitecto) |
| D4 | Guarda de consistencia en el punto de validez (reflog de `HEAD`, `HEAD`, identidad del índice e `index.lock` sin cambios desde el inicio) | Cubre el `git` que empieza durante la lectura sin depender de la latencia del motor. Concreta "descarte si llega un evento de Git durante la lectura" |
| D5 | Ancla posterior como captura `observation` con `cause_operation`, en lugar de una tabla de causalidad | No cambia el esquema ni la cadena del oplog; el ancla es útil por sí misma |
| D6 | Un hilo de captura por daemon, capturas en serie | El almacén ya serializa por repo; 23 ms de mediana por captura (SPIKE-TMC-001). Simple y suficiente para M1 |
| D7 | Detección completa (sin rutas del motor) | La interfaz de rutas con continuidad (escalón 2) sigue pendiente en el motor (TS-GRP-002/003); la detección completa nunca omite nada |
| D8 | `ENOSPC` = captura fallida, sin cuota propia; suelo de espacio libre de SEC-TMC-12 antes de cada captura | La cuota y el hueco "sin espacio" los entrega US-TMC-022. El suelo evita que la captura llene el disco del usuario (ajuste 3 del Arquitecto) |
| D9 | El e2e del criterio 3 atribuye el `reset` por la muestra S3, con un hook `reference-transaction` que mantiene vivo el `git` un momento | Es el mismo recurso que usan los tests de US-GRP-007 con un commit. El registro (US-GRP-009) solo atribuye a un "otro agente" registrado, no a Claude Code detectado |
| D10 | Generación del almacén del motor en `store_meta` y frontera en la Time Machine | Sin ella, un almacén del motor recreado reinicia la secuencia y el destino de un evento podía ser una captura antigua (bloqueante B2) |
| D11 | Protocolo 7 para el tipo `reset`, sin entregarlo a clientes anteriores | Un `raptor-mcp` o una CLI anteriores no saben leerlo (ajuste 4) |

## 4. Plan de tests

`crates/core/tests/us_tmc_004.rs` (macOS, como sus hermanos del canal): fixture del testkit, perfil temporal, daemon real en proceso con `Q` corto, Git real y cliente por el socket. Sin esperas fijas: cada paso espera un estado con plazo.

| Escenario Gherkin | Test |
|---|---|
| Una edición fuera de GitRaptor queda capturada | `an_edit_outside_gitraptor_is_captured`: `api.rs` modificado y `util.rs` nuevo en `feat-login`; aparece un punto `observation` con los dos |
| Un reset destructivo con Git crudo deja recuperable el último estado capturado | `a_raw_reset_leaves_the_last_capture_restorable`: tras el `reset --hard` hay un evento `reset`; el último punto anterior sigue disponible, es `observation` y tiene `api.rs` modificado; `raptor undo` (canal) lo recupera |
| Ignorados y credenciales | `ignored_files_and_credentials_are_not_captured`: ni `.env` ni `deploy.pem`; `deploy.pem` declarado `credential` |
| Archivo por encima del tope | `a_file_over_the_cap_makes_the_capture_partial`: `api.rs` dentro, `dump.bin` (51 MB disperso) fuera y declarado `too-large` |
| Una captura que falla no se presenta como protegida | `a_failed_capture_is_not_presented_as_protected`: fallo inyectado (`ENOSPC`); `tm_capture_failed` en el log; ningún punto completo del worktree; un undo del `reset` posterior responde `target-unavailable` y el repo no cambia |

Además: `a_reset_that_moves_no_branch_is_a_reset_event` (`watch.rs`: `reset` sin mover rama, sin duplicar `branch-update`); `a_capture_that_stopped_being_consistent_is_discarded_without_a_row` (`tm_store_capture.rs`); unitarios de la calma por estado (reflog y `HEAD` pendientes de persistir, `index.lock`, respaldo por tiempo), de `GitState`, de los tipos que se deshacen y de las marcas. El escenario 2 comprueba además el ancla y que el eco del undo no entra en la pila (un segundo undo responde `nothing-to-undo`). Los tests de US-TMC-001/002 siguen en verde con la marca común y el ancla (`consecutive_undos_walk_back` destapó que el cambio de rama lo nombra el worktree y no el repo: la calma compara también `HEAD`).

**E2E del criterio 3 de M1** (`apps/cli/tests/raw_git_undo.rs`): binario `raptor` como daemon y como cliente, Claude Code simulado (`raptor-fake-agent`, como en US-GRP-007) en `feat-login`, que modifica un archivo con seguimiento y crea uno nuevo; se espera al punto `observation`; hace `git reset --hard` con Git crudo (D9); el evento `reset` queda atribuido a Claude Code; el desarrollador ("sin atribuir") no puede deshacerlo (`confirmation-required`, worktree intacto); el propio agente ejecuta `raptor undo --json` desde su shell, deshace el `reset` con destino la captura por observación y recupera los dos archivos (contenido exacto); un segundo `raptor undo` responde "nothing to undo".

## 5. Verificación

macOS (Apple Silicon): `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y el banco del motor (`cargo bench -p gitraptor-cli --bench engine`) con la captura continua activa (bloqueante B4: el p95 del motor no empeora). Linux y Windows: los tests del canal son solo macOS, como sus hermanos; el servicio compila en los tres. **Pendiente: etapa de validación multiplataforma** (incluidos `Q`/`M`, ⚠️ ASSUMPTION fuera de macOS).

## 6. Pendientes y riesgos

| Pendiente | Dónde |
|---|---|
| Rutas del motor con marca de continuidad (escalón 2) para captura incremental | TS-GRP-002/003; hoy cada captura hace detección completa |
| Cuota del almacén y hueco "sin espacio" | US-TMC-022 |
| Gate de INF-GRP-002 con la Time Machine activa en Linux (CI) y en Windows | La captura corre siempre en el daemon, así que el banco de CI ya la incluye; Windows: etapa multiplataforma |
| Deshacer `push`, crear o borrar ramas y worktrees | US-TMC-009/014 |
| Timeline con nivel, atribución y huecos | US-TMC-006/007/008 |

**Riesgos residuales**: R2 (lo editado en el último `Q`/`M` antes de un `git` destructivo). Sin reflog de `HEAD` (`core.logAllRefUpdates=false`, backend reftable) no hay evento `reset` y la calma es por tiempo. `git clean`, `git restore` y `git checkout -- .` no escriben reflog ni índice y no son eventos (su efecto queda en la siguiente captura; lo anterior sigue restaurable desde el almacén). Un Git crudo de otro actor en el mismo worktree entre el fin de una operación y su ancla se toma como su eco. El tramo entre que `git` escribe el índice y su reflog es de microsegundos: una captura que empiece y termine dentro llevaría una marca anterior (no observado).

## 7. Validación del Arquitecto (2026-10-06)

Veredicto: **aprobada con ajustes**, con cuatro bloqueantes. Todo está incorporado:

1. **B1**: la calma por tiempo no evitaba el caso que decía evitar (notificaciones tardías). Ahora es por estado: reflog de `HEAD` y `HEAD` contra lo persistido (§ 2.3, D3) y la guarda compara el mismo estado (D4).
2. **B2**: las marcas solo se comparan dentro de una generación del almacén del motor (§ 2.2, D10).
3. **B3**: la marca de intención de toda operación protegida y de todo undo es en calma; sin calma, `git-busy` / `repo-busy` (§ 2.3).
4. **B4**: el presupuesto del motor se verifica aquí, con el banco del motor y la captura activa (§ 5).

No bloqueantes incorporados: ancla acotada y con el repo tomado (§ 2.5); el evento más reciente de un tipo no soportado da `raw-git-not-covered` (§ 2.6); suelo de espacio libre (D8); protocolo 7 (D11); enmiendas de ADR-TMC-003 y ADR-TMC-004; límites declarados (§ 6). D9 cambió al implementar: el registro no atribuye a Claude Code detectado, así que el e2e usa la muestra S3 como US-GRP-007.
