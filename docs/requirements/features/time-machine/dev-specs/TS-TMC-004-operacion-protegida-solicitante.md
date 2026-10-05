---
id: DS-TS-TMC-004
title: "Dev Spec — Operación protegida y resolución del solicitante en el canal"
type: dev-spec
status: review
feature: time-machine
domain: GRP
story: TS-TMC-004
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-TMC-004, ADR-TMC-005, ADR-TMC-002, ADR-TMC-003, ADR-GRP-005, ADR-GRP-012, ADR-GRP-013]
  nfrs: [NFR-01, NFR-02, SEC-TMC-03, SEC-TMC-07, SEC-TMC-12, SEC-TMC-15, SEC-12, SEC-14]
  deps: [DEP-MCP-3]
tags: [time-machine, canal, contrato, solicitante, snapshot-previo, reto, confused-deputy, crates-core, crates-api]
---

# Dev Spec — TS-TMC-004: operación protegida y solicitante

Blueprint compacto de [TS-TMC-004](../technical-stories/TS-TMC-004-operacion-protegida-solicitante.md). Fuentes: ADR-TMC-004 § 1 (operación protegida), ADR-TMC-005 § 1 y § 3 (solicitante y reto), ADR-GRP-005 § 6 (controles de los comandos reservados) y DEP-MCP-3 (confused deputy, `docs/requirements/features/mcp/context.md`). Implementada en esta misma rama.

**Estado `review`**: falta la verificación manual de la TS (un `raptor undo` desde un Claude Code real, que necesita US-TMC-002) y la integración con TS-TMC-003 y con el ejecutor de F-001-02/05, que se construyen en otras ramas.

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `crates/api/src/methods.rs` | Métodos nuevos con su clase de escritura (`RepoWrite`) |
| `crates/api/src/timemachine.rs` | Parámetros, resultados y proyecciones MCP; validación de formato (duración, id de agente, id de snapshot, nombre de operación) |
| `crates/core/src/channel/requester.rs` | Resolución del solicitante por ascendencia, multiplexor y marcas del ejecutor; elegibilidad para confirmar |
| `crates/core/src/channel/marks.rs` | `ExecutorMarks`: procesos lanzados por una operación (DEP-MCP-3) |
| `crates/core/src/channel/authz.rs` | El recorrido trata un proceso marcado como descendiente del ejecutor |
| `crates/core/src/timemachine/protected/mod.rs` | `ProtectedOperation`: intención, snapshot previo con tiempo máximo, ejecución, registro |
| `.../protected/challenge.rs` | Reto de un solo uso ligado a conexión, proceso y hash del plan, caduca a los 60 s |
| `.../protected/scope.rs` | Ámbito por MCP desde el cwd del llamante, allowlist y "no existe" para ids de otro repo |
| `crates/core/src/channel/conn.rs` | `operation.run` y `requester.resolve` en el canal; tabla `write_route` (único enrutado de los métodos que escriben); los comandos de la Time Machine validan y responden `NOT_IMPLEMENTED` con su historia |
| `crates/core/src/channel/peer.rs` | `ProcInfo.pgid` y `ProcSource::pids_of` (lista de procesos del usuario, para el multiplexor) |

## 2. Contrato del canal

| Método | Escribe en el repo | MCP | Implementa |
|---|---|---|---|
| `operation.run` | Sí, como operación protegida | Sí | Esta TS (el ejecutor real llega con F-001-02/05) |
| `timemachine.undo` | Sí, operación protegida de la Time Machine | Sí | US-TMC-002, 010, 011 |
| `timemachine.redo` | Sí, ídem | No (Q-MCP-11) | US-TMC-003 |
| `timemachine.restore` | Sí, ídem | No (Q-MCP-11) | US-TMC-009 |
| `timemachine.snapshot` | No (solo el almacén) | No | US-TMC-005 |
| `timemachine.timeline` | No | No (Q-MCP-11) | US-TMC-006 |
| `requester.resolve` | No | Sí | Esta TS |

- `MethodSpec` gana `writes: RepoWrite` (`None`, `Protected`, `TimeMachine`). El test de contrato recorre `METHODS` y exige que solo `operation.run` y undo, redo y restauración declaren escritura. En el canal, `write_route` es la única puerta de esos cuatro métodos: `operation.run` pasa por `ProtectedOperation` y los demás, hasta su historia, responden `NOT_IMPLEMENTED` **después** de validar y **antes** de escribir nada en el oplog.
- El solicitante **no** es un parámetro: ningún tipo de parámetros tiene campo de actor, agente o humano, y `deny_unknown_fields` rechaza uno inventado. Lo que un cliente declare no cambia nada.
- El canal sale del perfil de la conexión: una conexión MCP es siempre `mcp`. Una conexión completa puede declarar `surface` (`cli`, `tui`, `hook`) como etiqueta; por MCP ese campo se rechaza.
- `operation.run { operation, worktree, args }`: `operation` es un nombre del catálogo de ADR-CKP-002, que **no** se inventa aquí: solo se valida su forma (`[a-z][a-z0-9-]{0,63}`) y el ejecutor dice si existe. `args` es un objeto acotado (≤ 32 claves, ≤ 4 KiB) que valida el ejecutor.
- Respuestas: campos estructurados. Rutas y refs viajan como `Untrusted`; la proyección MCP no lleva rutas ni contenido de archivos y recorta el texto no confiable a 256 bytes (SEC-12). `requester.resolve` por MCP devuelve solo el actor y el canal: sin la vía ni si puede confirmar, para no ofrecer un oráculo sin auditoría.
- Códigos nuevos: `PRIOR_SNAPSHOT_FAILED` (con `reason` y `operation_id`), `NOT_FOUND`, `SCOPE_REFUSED`, `OPERATION_FAILED` e `IDENTITY_UNVERIFIED`.

## 3. Operación protegida

`ProtectedOperation::run` es el único camino (ADR-TMC-004 § 1): (1) `record_operation` (intención, solicitante congelado, canal), (2) snapshot previo `guaranteed-prior` del ámbito con tiempo máximo, (3) transición `prior-snapshot` + `ready`, (4) el paso de ejecución, (5) `finished` o `interrupted`.

- **Punto de enganche**: trait `ProtectedStep`. Lo implementan el ejecutor de operaciones de usuario (F-001-02/05) y el aplicador de TS-TMC-003; ninguno se construye aquí. Los tests usan dobles.
- **Snapshot previo**: trait `PriorSnapshotter`; la implementación de producción envuelve `SnapshotStore::capture` con nivel `GuaranteedPrior`. Corre en un hilo; si pasa el tiempo máximo (10 s por defecto), la operación se aborta. La captura tardía queda como un punto más, nunca como previo de esta operación.
- **Fallos** (`PriorFailure`): `no-space` (`ENOSPC`/`EDQUOT` o cuota), `store-unavailable`, `timeout`, `daemon-stopping`, `capture-failed`. Siempre `aborted` en el oplog con el motivo y el paso de ejecución no corre.
- **Daemon muerto a la fuerza**: el cliente pierde la conexión sin respuesta. Lo correcto en la CLI es "estado desconocido, consulta el oplog", nunca "no se ejecutó": si `ready` ya estaba anotado el paso pudo empezar. La recuperación de TS-TMC-002 cierra la operación al arrancar.
- **`StepCtx`** da al paso el id de la operación y del snapshot previo, el solicitante congelado, el canal y la señal de parada; solo anota su propio progreso (`applying`) y sus hijos (`spawn`/`wait`). El paso no escribe el oplog por otra vía.
- **Daemon deteniéndose**: se comprueba antes del snapshot y antes del paso.

## 4. Solicitante (ADR-TMC-005 § 1)

Se resuelve en cada petición, en el daemon, con lo que dice el kernel del par del socket:

1. La identidad del proceso `(pid, inicio)` debe ser la del `accept`; si no, la petición se rechaza (`identity-unverified`).
2. Se recorre la ascendencia con las reglas de `authz::walk`: un antecesor más joven que su hijo es un PID reutilizado y rompe el recorrido (no confirmable). **El recorrido se corta en el daemon**: si lo lanzó `raptor-mcp` bajo un agente, lo que está por encima del daemon no dice nada del llamante; un descendiente del daemon sin marca es "sin atribuir" y no confirmable.
3. **Marca del ejecutor**: si un eslabón está marcado, el solicitante es el de la operación que lo lanzó (DEP-MCP-3).
4. **Agente**: el primer eslabón que es un agente (S1, `AgentMatcher`) da "agente X" con origen `detected`, sesión `<pid>:<inicio>`.
5. **Multiplexor**: si un eslabón es un servidor de `tmux`, `screen`, `zellij`, `abduco` o `dtach`, se buscan los Claude Code vivos del usuario que descienden de ese mismo servidor. Uno: el cliente es ese agente. Varios: "sin atribuir" y no confirmable. Ninguno descendiente pero alguno vivo (un agente fuera de tmux puede manejarlo como cliente con `tmux new-window -t`): "sin atribuir" y no confirmable. Ningún agente vivo: las reglas normales. Lista no disponible: "sin atribuir" y no confirmable (fail-closed).
6. En cualquier otro caso: "sin atribuir". Nunca "humano".

Resultado: `Actor` del contrato, `Requester` del oplog, la vía (`ancestry`, `multiplexer`, `executor`) y si puede confirmar.

## 5. Reto de confirmación (SEC-TMC-03)

- **Emisión**: solo si el par pasa `check_reserved` (terminal de control, fuera del árbol de un agente y del ejecutor, líder de sesión limpio), no está contaminado por multiplexor y la plataforma no es Windows.
- **Ligado** a conexión, `(pid, inicio)` y hash SHA-256 del plan que calcula el daemon, serializado de forma canónica (nunca uno que envíe el cliente); 128 bits de `getrandom`; caduca a los 60 s. Un solo reto vivo por conexión.
- **Canje** de un solo uso: reutilizado, de otra conexión, de otro proceso, con el plan cambiado o caducado, rechazado con su motivo. Un fallo de canje consume el reto. Al canjear se repiten `check_reserved` y la regla del multiplexor.
- El canal no expone todavía el reto: lo usa US-TMC-013 con el plan que calcule el daemon.

## 6. MCP (SEC-TMC-07, SEC-TMC-15)

- Repo y worktree salen del cwd del llamante (`peer::process_cwd`), nunca de un parámetro. Sin cwd legible, se rechaza (en macOS no hay lectura segura del cwd todavía: fail-closed).
- Antes de un undo o una restauración el repo debe estar en la allowlist (trait `McpAllowlist`). Hasta que exista su almacén (DEP-MCP-4) la de producción no admite ninguno.
- Un id de snapshot u operación de otro repo responde "no existe", igual que uno inexistente.
- "Sin atribuir" por MCP se rechaza antes de calcular nada en undo y restauración (TQ-7 → a); lo aplican esas historias con `ScopeCheck::require_attributed`.

## 7. Marcas del ejecutor (DEP-MCP-3)

`ExecutorMarks` guarda `(pid, inicio)` y el **grupo de procesos** de cada hijo que lanza un paso con `StepCtx::spawn` (que lo pone en su propio grupo), con la operación y su solicitante. Las marcas duran hasta que **se cierra la operación**, no hasta que termina el hijo: un nieto huérfano del mismo grupo sigue cubierto. Un grupo solo marca procesos que arrancaron después de abrirse la operación (un id de grupo reutilizado no cuenta). Si no se puede leer la identidad del hijo recién lanzado, se mata (fail-closed). El paso anota `child-started`/`child-ended` en el diario. En el canal:

- `authz::walk` trata un eslabón marcado como descendiente del ejecutor: comando reservado rechazado con `daemon-descendant`, también si el hijo se reparentó fuera del árbol del daemon.
- `requester.resolve` y `operation.run` atribuyen al hijo el solicitante de la operación.
- Un nieto que cambia de grupo de procesos y se desacopla escapa: es el riesgo residual aceptado de ADR-GRP-005 § 6. En Linux, hacer al daemon *subreaper* (`PR_SET_CHILD_SUBREAPER`) lo acotaría: Pendiente: etapa de validación multiplataforma.

## 8. Decisiones

Decisión del orquestador (2026-10-04), validada por el Arquitecto, que pidió ajustes y están incorporados (§ 2, § 3, § 4, § 5 y § 7):

1. **Un solo trait de ejecución** (`ProtectedStep`) para operaciones de usuario y para el aplicador: un único camino, y TS-TMC-003 y F-001-02/05 se enchufan sin tocar el pipeline. El paso no escribe el oplog salvo su progreso y sus hijos.
2. **Comandos de la Time Machine declarados con validación y `NOT_IMPLEMENTED`**: la TS exige exponerlos con validación estricta; planificar undo, redo y restauración es de sus historias. Responden tras validar y antes de escribir en el oplog, sin dejar intenciones huérfanas.
3. **`requester.resolve`**: método de lectura que devuelve cómo ve el daemon al llamante. Sirve al plan de la CLI y hace observable la atribución en los tests y en el dogfooding. Por MCP, solo el actor.
4. **Multiplexor ambiguo, ilegible o con cualquier agente vivo → no confirmable**: nunca elige un agente al azar ni concede la confirmación con dudas.
5. **Allowlist MCP deny-all en producción** hasta DEP-MCP-4.
6. **`operation.run` sin ejecutor cableado** responde `NOT_IMPLEMENTED` (F-001-02): el daemon de producción aún no ejecuta operaciones de usuario.
7. **El recorrido del solicitante se corta en el daemon** (riesgo bloqueante señalado por el Arquitecto: el daemon hace `setsid` sin doble fork, así que si lo arrancó `raptor-mcp` su padre es el agente).
8. **Marcas por grupo de procesos, hasta el cierre de la operación** (no por hijo directo hasta que termina).

## 9. Plan de tests (todos con directorios y procesos temporales; NFR-01)

| Criterio de la TS | Test |
|---|---|
| Contrato: solo la operación protegida y la Time Machine escriben | `api::methods::only_protected_paths_write`; `channel` enruta exactamente esos métodos por `ProtectedOperation` |
| Garantía: almacén sin espacio | `protected::no_space_aborts_without_running_the_step` (oplog real, `ENOSPC` del doble) |
| Garantía: daemon detenido | `protected::stopping_daemon_aborts`; integración: el cliente recibe el motivo |
| Garantía: tiempo máximo | `protected::timeout_aborts` |
| Solicitante por CLI y MCP desde un Claude Code simulado | `apps/cli/tests/protected_process.rs` (daemon real, agente simulado `raptor-fake-agent`) y unitarios con árboles sintéticos |
| Sin agente → "sin atribuir"; nunca humano | ídem + test de esquema |
| No confianza en el cliente | parámetros con `actor`/`agent`/`human` rechazados; resolución sin cambios |
| Validación previa | duraciones, ids de agente y de snapshot fuera de formato → `INVALID_PARAMS` sin oplog escrito |
| Ascendencia y reto | `setsid` sin pty, `tmux new-window` + `send-keys`, PID reutilizado, reto reutilizado, de otra conexión, plan cambiado, caducado |
| MCP | repo fuera de la allowlist rechazado; snapshot de otro repo "no existe" |
| Salida | rama con escapes: `sanitized()` limpio y `{"untrusted": …}` recortado en MCP |
| DEP-MCP-3 | `channel_protected::a_child_of_the_operation_cannot_use_a_reserved_command`: el paso lanza `sh -c '… &'`, el nieto queda huérfano (fuera del árbol del daemon), pide `daemon.stop` y es rechazado con `daemon-descendant`; su `requester.resolve` da la vía `executor` con el solicitante de la operación. Unitarios en `requester` y `marks` |

## 10. Fuera de alcance (y a quién pertenece)

- Capa de escritura y aplicador: TS-TMC-003 (implementa `ProtectedStep`).
- Ejecutor de operaciones de usuario y catálogo: F-001-02/05 y ADR-CKP-002.
- Planificar undo, redo y restauración, regla base de permisos y confirmación interactiva: US-TMC-002/003/009/010/011/013.
- Política de Guardrails: US-TMC-021. Comando para hooks: US-TMC-005.
- Reserva de disco del previo (SEC-TMC-12): la aplica el almacén; aquí solo se traduce `ENOSPC` y la cuota a `no-space`.

## 11. Pendientes multiplataforma

- Lista de procesos del usuario para el multiplexor en Linux (`/proc`): implementada, sin verificar. Pendiente: etapa de validación multiplataforma.
- Windows: sin canal (TS-GRP-004) ni reto (ADR-TMC-005 § 3); todo compila con `cfg`. Pendiente: etapa de validación multiplataforma.
- `process_cwd` en macOS devuelve `None`: el ámbito por MCP queda en fail-closed hasta tener una lectura segura (US-GRP-009).
