---
id: DS-TS-CKP-002
title: "Dev Spec — Catálogo de operaciones de usuario y ejecutor del daemon"
type: dev-spec
status: review
feature: cockpit
domain: GRP
story: TS-CKP-002
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-CKP-002, ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007, ADR-GRP-005, ADR-GRP-009]
  nfrs: [NFR-01, NFR-02, NFR-07, SEC-02, SEC-05, SEC-10, SEC-11, SEC-12]
  deps: [DEP-CKP-7, DEP-MCP-2, DEP-MCP-3, DEP-MCP-5]
tags: [cockpit, mcp, catalogo-operaciones, ejecutor, plan, huella, cerrojo, cola, capa, guardrails, barrera, confused-deputy, crates-api, crates-core, crates-git]
---

# Dev Spec — TS-CKP-002: catálogo y ejecutor

Blueprint compacto de [TS-CKP-002](../technical-stories/TS-CKP-002-catalogo-ejecutor.md), implementado en la rama `feat/TS-CKP-002-operations-executor`. Sigue ADR-CKP-002 § 1 a § 6, § 11 y § 12. Las decisiones que el ADR dejaba abiertas están en el § 8, validadas por el Arquitecto y el PO.

**Estado `review` (implementada sin cablear)**: catálogo, contrato, ejecutor, cola, revalidación, barrera, validación de la ruta nueva (H-02), invocación de Git y lectura de precondiciones están implementados y probados en macOS. El cableado de producción del backend lo hace **US-MCP-008** (§ 9): hasta entonces "una sola vía" no llega a producción. Linux y Windows: **Pendiente: etapa de validación multiplataforma** (§ 10).

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `crates/api/src/catalog.rs` | Catálogo v1: `OperationId`, clase, gobernanza, marcas Cockpit/MCP e historia dueña. Argumentos tipados con `deny_unknown_fields` (`create-worktree` sin `path` por MCP, H-02). Reglas de rama nueva (L-02) y del saneado de la plantilla. Parámetros y resultados de `describe`, `prepare`, `run` y `cancel`. Avisos, `RejectReason`, `OperationOutcome` y datos de los eventos de operación |
| `crates/api/src/methods.rs`, `rpc.rs`, `event.rs`, `lib.rs` | Métodos `operation.describe`, `operation.prepare` y `operation.cancel`. `operation.run` pasa a ser la fase de ejecutar. Código `-32014 OPERATION_REJECTED`. Eventos `operation.queued`, `operation.started` y `operation.finished`. `PROTOCOL_VERSION = 4` (`API_VERSION` 4.0.0) |
| `crates/core/src/repo_lock.rs` | Cerrojo de escritura por repo en un módulo neutro, compartido con el aplicador. `timemachine::repo_lock` lo reexporta. `try_lock` no espera; `lock_queued` espera en cola FIFO acotada y nunca deja que un `try_lock` se cuele |
| `crates/core/src/executor/mod.rs` | `Executor`: preparar, ejecutar y cancelar. Capa (`layer_for`), admisión por capa (`admit`), regla base de permisos (`permission`), huella SHA-256, `planId`, retos, cola, revalidación y envoltorio del paso (registro para Cancelar y tiempo máximo de la capa `mcp`) |
| `crates/core/src/executor/facts.rs` | `RepoFacts`, desde la lectura `preflight`, y las precondiciones comunes en su orden (`check_common`) |
| `crates/core/src/executor/gate.rs` | `GuardrailsGate` (evaluar y cerrar el plan una vez) y `NoGuardrails` (producción hasta TS-CKP-003, cerrado ante fallos) |
| `crates/core/src/executor/git.rs` | `run_git`: único usuario de `user_ops`. Lanza el `git` como hijo marcado y lo interrumpe con SIGINT a su grupo (Cancelar o `time-limit`) |
| `crates/core/src/timemachine/protected/scope.rs` | `ProtectedBackend` ampliado: `write_lock_key` y `facts` obligatorios, `plan_op` y `step(&StepPlan)` |
| `crates/core/src/timemachine/protected/mod.rs` | `StepCtx::spawn_with` (lanzamiento opaco con la barrera abierta) y `child_ended`. `StepOutput.outcome` y `git_output` |
| `crates/core/src/channel/marks.rs`, `requester.rs` | Barrera de arranque del lado del lector (I-02, § 5) |
| `crates/core/src/channel/conn.rs`, `mod.rs` | Rutas de los cuatro métodos, `exec_error`, id de conexión (los planes se borran al cerrarla), `ProtectedWiring::new` con el `Executor` y `test_layer_override` |
| `crates/core/src/executor/new_path.rs` | Ruta nueva de `create-worktree` (H-02): `lstat` de cada componente, padre del uid y no escribible por el grupo ni por otros, fuera de todo `.git`, del perfil y de los worktrees observados, y último componente inexistente. La historia dueña la llama al preparar y otra vez bajo el cerrojo |
| `crates/git/src/preflight.rs` | Lectura de precondiciones e identidad: raíz, `.git`, directorio común, `(dev, inode)`, `gitdir` bidireccional, HEAD, operación en curso, locks de Git (nunca se borran), worktree bloqueado, `info/grafts` y ramas sacadas en otros worktrees |
| `crates/git/src/user_ops.rs` | Invocación de operaciones de usuario: lista cerrada (`UserOp`), `UserGitCommand` opaco, `-c` fijos del § 6, entorno desde cero, variables de sesión validadas (M-01) y editor de rechazo (L-01) |

## 2. Contrato (protocolo 4)

| Método | Reservado | MCP | Parámetros → resultado |
|---|---|---|---|
| `operation.describe` | No | Sí, solo las de marca MCP | — → `{catalog_version, operations: [{id, class, governed, cockpit, mcp, args_schema}]}` |
| `operation.prepare` | No | Sí | `{operation, worktree?, args, surface?, session_env?}` → `{plan_id, catalog_version, operation, layer, requester, fingerprint, warnings, decision, challenge?, expires_in_ms, diagnostics}`. Por MCP, la proyección sin `requester`, `layer`, `challenge` ni `diagnostics` |
| `operation.run` | No | Sí | `{plan_id, accepted_warnings, confirmation?}` → `OperationRunResult` con `outcome`, `layer` y `git_output`. `git_output` solo va con la capa `cockpit`; nunca por MCP |
| `operation.cancel` | No | No | `{operation_id}` → `{requested}`. Exige capa `cockpit` |

- Un rechazo es `-32014` con `data.reason`. Los motivos son: `state-changed`, `plan-unknown`, `not-available-for-layer`, `unattributed-without-cockpit`, `executor-descendant`, `warnings-mismatch`, `confirmation-required`, `challenge-invalid`, `foreign-work`, `other-session-present`, `operation-in-progress`, `detached-head`, `git-busy`, `worktree-locked`, `branch-checked-out-elsewhere`, `grafts`, `repo-identity-changed`, `guardrails-denied`, `new-path-refused`, `queue-full` y `daemon-stopping`. Ninguno escribe en el oplog.
- Una operación sin su historia responde `-32004` con `implemented_by` = la historia dueña, al preparar.
- Más planes de los permitidos: `-32006`.
- `catalog_version` = 1. Va en `describe`, en el plan y en la huella; no va en `hello`, cuya forma no cambia entre versiones.

## 3. Preparar (sin efectos)

Orden:
1. Un descendiente del ejecutor se rechaza con `executor-descendant`.
2. Admisión por capa. Un "sin atribuir" con capa `mcp` se rechaza con `unattributed-without-cockpit`. Sin la marca de la capa: `not-available-for-layer`.
3. Argumentos tipados.
4. Variables de sesión validadas (M-01); lo que no pasa se omite con un diagnóstico.
5. Hechos del repo y precondiciones comunes. El orden es operación en curso → HEAD separado → locks → worktree bloqueado → rama sacada en otro worktree → grafts.
6. La parte propia de la operación (`plan_op`, de su historia).
7. "Otra sesión presente": rechazo en un rebase con capa `mcp`, aviso con capa `cockpit`.
8. Regla base de permisos y, si toca trabajo ajeno, un reto ligado a la huella.
9. Huella, vista previa de Guardrails y `planId`.

La **huella** es el SHA-256 del JSON del plan: versión del catálogo, operación, argumentos (el mensaje de commit entra solo como longitud y resumen), capa, actor y solicitante, repo, worktree, hechos, parte de la operación y nombres de las variables de sesión. La marca de secuencia del motor no entra, porque cambia sin que cambie el plan.

## 4. Ejecutar

1. `executor-descendant` antes de cualquier cerrojo.
2. El plan se consume en cualquier intento de su conexión. El de otra conexión, uno desconocido y uno caducado dan la misma respuesta, `plan-unknown`.
3. Los avisos deben coincidir exactamente.
4. Si el plan lleva reto, se canjea, y los controles se vuelven a pasar.
5. Cola del cerrojo, con `operation.queued` y la posición. Con la cola llena: `queue-full`. Si el daemon se para: `daemon-stopping`.
6. Bajo el cerrojo:
   - El solicitante y la capa se resuelven otra vez; si difieren, `state-changed`.
   - Hechos nuevos: si la identidad no es la del plan, `repo-identity-changed`.
   - Plan rehecho: una precondición que ahora falla, o una huella distinta, dan `state-changed`.
7. La decisión que cuenta. Todo lo que no sea "permitir" da `guardrails-denied`.
8. La operación protegida: intención, snapshot previo garantizado, paso y registro. El paso se envuelve para registrar su `operation_id`, que es lo que usa Cancelar, y publicar `operation.started`. Con la capa `mcp` corre además el tiempo máximo.
9. `operation.finished` con el desenlace y el cierre del plan (`record_close`), una sola vez por plan gobernado. También se cierra al caducar o al cerrarse la conexión.

## 5. Barrera de arranque (I-02) y descendientes (H-01)

- `StepCtx::spawn_with` abre un *spawn pendiente* antes de lanzar y lo cierra al marcar el hijo.
- Durante ese intervalo, un llamante que desciende del daemon sin marca espera el registro, como mucho 2 s (`REGISTRATION_WAIT`), y se resuelve otra vez. Si se pasa del tope, su identidad no se verifica y se rechaza.
- Así un hook que se conecta en cuanto arranca el `git` se atribuye al solicitante del plan y nunca resuelve "sin atribuir" en esa ventana.
- La barrera del lado del lector se queda en Rust seguro. El *spawn* suspendido de ADR-CKP-002 § 4 necesita `unsafe` o `posix_spawn`, que prohíben los lints del workspace (§ 8, D3).
- Los rechazos `executor-descendant` cubren `prepare`, `run` y `cancel`. Los comandos reservados ya los rechaza el canal con `daemon-descendant`.

## 6. Entorno de `git` (ADR-CKP-002 § 6)

- `--git-dir` y `--work-tree` siempre explícitos; el cwd es la raíz del worktree.
- Editores (`core.editor`, `sequence.editor`, `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`): `raptor-no-editor` si su ruta absoluta está limpia y, si no, `/usr/bin/false`. Nunca `:`.
- `-c`: `protocol.allow=never`, `credential.helper=`, `submodule.recurse=false`, `core.useReplaceRefs=false`, `gc.auto=0`, `maintenance.auto=false`, `rebase.updateRefs/autoStash/autoSquash=false`, `core.fsmonitor=false`, `core.pager=cat`, `diff.external=` y trace2 vacío.
- Variables fijas: `GIT_MERGE_AUTOEDIT=no`, `GIT_TERMINAL_PROMPT=0`, `GIT_NO_REPLACE_OBJECTS=1` y `GIT_LITERAL_PATHSPECS=1`.
- Entorno construido desde cero: `HOME`, el `PATH` validado o `/usr/bin:/bin`, y las variables de sesión que pasaron.
- `PATH`:
  - Como mucho 64 entradas y 4 KiB.
  - Solo entradas absolutas, existentes y no escribibles por el grupo ni por otros.
  - Ninguna dentro del repo ni de su worktree.
- `SSH_AUTH_SOCK`: un socket del uid.
- `GNUPGHOME`: un directorio del uid con permisos 0700.
- Locales: `^[A-Za-z0-9_.@-]+$`, como mucho 64 bytes.
- Lanzamiento:
  - El hijo tiene su propio grupo de procesos.
  - stdin va a nulo o, en `commit`, recibe el mensaje y se cierra.
  - stdout y stderr se capturan con un tope de 256 KiB por stream.
  - El hijo solo hereda los fds 0, 1 y 2: la biblioteca estándar abre todo lo demás con `CLOEXEC`.
  - Sin terminal de control: el daemon no tiene ninguna.
- Variantes inexistentes: `--amend`, `--no-verify`, `--allow-empty`, `--exec` y `--force` doble. Las refs se actualizan siempre con el nombre completo `refs/heads/` y el valor viejo.

## 7. Plan de tests (directorios, repos y perfiles temporales; NFR-01)

| Criterio de la TS | Test |
|---|---|
| Catálogo, marcas, esquema, `deny_unknown_fields`, ramas (L-02), commit literal | `crates/api/src/catalog.rs` (6 tests) |
| Una vía (snapshot previo; almacén lleno → `aborted`) | `channel_protected::without_a_prior_snapshot_nothing_runs_and_the_client_gets_why`, `operation_run_takes_the_prior_snapshot_then_runs` |
| Frontera estática | `crates/core/tests/executor_boundary.rs`, `crates/git/tests/static_check.rs` |
| Capa (M-03) | `executor::tests::the_layer_comes_from_the_requester`, `what_each_layer_may_ask`, `channel_protected::layer_comes_from_the_daemon_without_the_override`, `mcp_scope_comes_from_the_caller` |
| Plan (M-04) | `channel_protected::a_plan_belongs_to_its_connection_and_runs_once` |
| Huella | `a_changed_plan_is_rejected_without_effects` (doble) y `an_external_commit_between_prepare_and_run_is_rejected` (git real) |
| Valor esperado | `user_ops_preflight::a_ref_update_with_a_stale_old_value_fails_whole` |
| Repo revalidado (M-05) | `user_ops_preflight::a_replaced_dot_git_changes_the_identity`, `channel_protected::a_replaced_repo_is_rejected_under_the_lock` |
| Precondiciones de Git y su orden | `user_ops_preflight::git_preconditions_are_read_without_touching_anything`, `facts::tests::preconditions_keep_their_order` |
| Serialización por repo y cola visible | `repo_lock::tests::*`, `channel_protected::operations_on_one_repo_are_serialized` |
| Descendientes (H-01, DEP-MCP-3) | `channel_protected::a_child_of_the_operation_cannot_use_a_reserved_command`: el hijo, tras un doble fork, se atribuye al solicitante; un reservado da `daemon-descendant`, y `prepare`, `run` y `cancel` dan `executor-descendant` |
| Barrera (I-02) | `marks::tests::a_pending_spawn_holds_the_barrier_until_dropped` y la espera en `requester::resolve` |
| Entorno, sin TTY, fds 0 a 2, mensaje fuera de argv | `user_ops::tests::*` y `user_ops_preflight::hooks_run_without_a_terminal_and_with_only_fds_0_to_2` |
| Variables de sesión (M-01) | `user_ops::tests::session_variables_are_validated` |
| Ruta nueva (H-02, Validación 19): enlace simbólico, padre escribible por el grupo, dentro de `.git` o de un worktree, ruta que aparece entre preparar y ejecutar | `executor::new_path::tests::a_new_worktree_path_is_checked` |
| Editor de rechazo (L-01) | `user_ops::tests::the_rejecting_editor_is_clean` |
| Guardrails (una decisión y un registro; cerrado ante fallos) | `channel_protected::guardrails_decide_before_any_effect` |
| Cancelar y tiempo | `channel_protected::cancel_reaches_the_running_operation`, `cancel_interrupts_a_hanging_hook` (hook real `sleep 60`), `git::tests::the_time_limit_interrupts_with_its_reason` |
| Flujo completo con Git | `channel_protected::a_real_worktree_is_created_through_the_executor` |

## 8. Decisiones

Todas son **decisiones del orquestador (2026-10-05), validadas por el Arquitecto**. La D11 la validó también el PO, con ajustes que ya están incorporados.

- **D1. `prepare`/`execute` frente a `operation.run`**, el punto que dejó abierto arch-cockpit. `operation.run` se conserva como la fase de ejecutar y sigue siendo la única ruta `RepoWrite::Protected`. Sus parámetros pasan a `{plan_id, accepted_warnings, confirmation?}`. Se añaden `operation.describe`, `operation.prepare` y `operation.cancel`; no se crea un `operation.execute`.
  - El cambio de forma sube el protocolo a 4 (API 4.0.0). El 3 ya lo había tomado US-GRP-012.
  - Los tests de TS-TMC-004 se migran a preparar y ejecutar con todas sus aserciones.
  - `catalog_version` va en `describe` y en el plan; la capa y el solicitante, en `prepare` y `requester.resolve`.
- **D2. Override de capa para tests**. `ProtectedWiring.test_layer_override` existe solo para tests, porque los clientes in-process descienden del daemon. Nunca se aplica a la vía `Executor`. `ProtectedWiring::new` lo deja en `None`.
- **D3. Barrera del lado del lector** (§ 5) en lugar del *spawn* suspendido. Ese queda pendiente en ADR-CKP-002 y es un requisito de la primera historia que lance `git` en producción.
- **D4. Cerrojo en `crate::repo_lock`**, con cola FIFO de 8. `write_lock_key` no tiene implementación por defecto: el backend de producción debe devolver la ruta del almacén, la clave del aplicador.
- **D5. Guardrails detrás de `GuardrailsGate`**. La vista previa sin motor es `not-evaluated`. `NoGuardrails` rechaza toda operación gobernada al ejecutar.
- **D6. `facts` y `plan_op` sin implementación por defecto**. `not-implemented` sale al preparar como `-32004`.
- **D7. `UserGitCommand` opaco**. Fija en su constructor el entorno, stdin, la captura y el grupo de procesos. La validación del `PATH` recibe las raíces que debe excluir.
- **D8. Planes**. `planId` de 128 bits; TTL de 60 s; como mucho 4 planes vivos por conexión (el quinto recibe `-32006`). El plan se consume en cualquier intento de `run`, y cada plan gobernado se cierra una sola vez.
- **D9. Eventos de operación**, sin argumentos ni salida de Git, y nunca a MCP.
- **D10. Motivos tipados** en `RejectReason` (§ 2), cerrados para esta versión del protocolo.
- **D12. Integración con US-TMC-001** (rebase del 2026-10-05), validada por el Arquitecto:
  - **`OperationCatalog`** es ahora la parte propia de cada operación: `plan_op` y `step(&StepPlan)`.
  - **`OperationsWiring`** gana el gate de Guardrails y el override de capa, que solo se aplica en builds de depuración.
  - **El ejecutor** construye la petición con `ProtectedRequest::for_step`, que valida el ámbito que declara el paso. Si el paso declara un worktree de otro repo: `SCOPE_REFUSED`, sin intención en el oplog. `OpPlan` ya no lleva `worktrees` ni `refs`; el ámbito sale del paso y solo de `StepPlan`.
  - **Los tests de US-TMC-001** pasan por las dos fases con ids del catálogo y conservan todas sus aserciones.
  - Se corrigió `registered_worktrees`, que no canonicalizaba el directorio común abierto desde un worktree enlazado.
- **D11. Alcance**, validado por el PO. La lógica de cada operación y el cableado de producción del backend quedan en sus historias dueñas (§ 9).

## 9. Fuera de alcance (y a quién pertenece)

Ajustes del PO (2026-10-05) incorporados: cada pendiente tiene un dueño con id.

- **Lógica propia de cada operación**: `plan_op` y `step` de producción. Pertenece a US-CKP-014 (merge), US-CKP-015 y US-MCP-018 (rebase `stop` y `atomic`), US-CKP-017 (descartar), US-CKP-018 y US-MCP-019 (crear worktree con la plantilla; llaman a `check_new_worktree_path` al preparar y bajo el cerrojo), US-MCP-009 (commit), US-MCP-008 (snapshot manual) y US-CKP-016 (abortar). Hasta entonces, `-32004` con `implemented_by`.
- **Cableado de producción**:
  - **Hecho al rebasar sobre US-TMC-001** (D12):
    - `DaemonBackend` implementa el `ProtectedBackend` ampliado: `repo_of` real con `StoreSnapshotter`, `facts` con `RepoFacts::read`, y la clave del cerrojo con `SnapshotStore::location`.
    - **Clave del cerrojo fijada por el Arquitecto**: la ruta canónica del almacén del repo, la misma que usa el aplicador de TS-TMC-003 (test `the_lock_key_is_the_appliers`).
  - **Falta, lo hace US-MCP-008**, la primera operación no gobernada; su bloqueo por la API de captura de la Time Machine sigue en pie (D-14 del MCP):
    - el `OperationCatalog` de producción;
    - poner `DaemonConfig.operations` en el daemon real;
    - las raíces observadas y el perfil, para el `PATH` y la ruta nueva;
    - el binario de Git resuelto.

  Hasta entonces, en producción `operation.prepare` responde `-32004` (`F-001-02`).
- **Motor de Guardrails, su registro y el actor de los `git` nietos**: TS-CKP-003. La excepción consciente con su ventana: US-CKP-019 (ADR-GRD-007).
- **Validaciones de la TS que necesitan una operación real o un arnés**, con dueño:
  - Captura de red y *partial clone* (M-02, Validación 16): suite "ejecutor" de INF-GRP-001. Los flags ya están en el argv y probados aquí.
  - Auditoría de `exec` (padre directo, `--git-dir` y `--work-tree`; Validación 2, dinámica): suite "ejecutor" de INF-GRP-001.
  - `refs/replace` con un merge real (L-04, Validación 25): US-CKP-014.
  - Caída del daemon a mitad de una operación: INF-TMC-001.
  - Tiempo máximo de la capa `mcp` con un agente real, a nivel de proceso (Validación 29): US-MCP-018. Aquí, test unitario del temporizador y Cancelar con un hook real.
  - Dos TUIs que descartan el mismo worktree ("ya no existe"): US-CKP-017. El mecanismo, plan rehecho bajo el cerrojo con `state-changed`, ya está probado aquí con dobles y con un commit externo real.
  - Instalación de `raptor-no-editor`: INF-GRP-004. Hasta entonces, el editor de rechazo es `/usr/bin/false`.
- **`NOT_IMPLEMENTED` con `implemented_by` en `operation.prepare`**: recogido en `api-contract-ipc.md`. Dueño del contrato: worker del canal (TS-GRP-004).
- **Cancelar mientras se espera en la cola**: solo se abandona si el daemon se para; la petición todavía no tiene `operation_id`. Dueño: US-CKP-023 (cola y confirmación en la TUI).

## 10. Pendientes multiplataforma

**Pendiente: etapa de validación multiplataforma.**
- **Windows**: `CREATE_NO_WINDOW` y consola, la interrupción con Ctrl-C al grupo (hoy no hace nada), el `FileId` del `.git` (hoy `None`), los permisos de `PATH`, `SSH_AUTH_SOCK` y `GNUPGHOME` (hoy no se comprueban), el editor de rechazo y la capa `cockpit` (sin controles equivalentes, TQ-14).
- **Linux**: el daemon como *subreaper*, la muerte del hijo con el daemon y un hook con doble fork.
- Todo compila con `cfg` en las tres plataformas, pero solo se ejecutó en macOS.
