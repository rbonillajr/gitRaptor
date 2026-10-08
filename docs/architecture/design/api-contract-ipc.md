---
id: API-GRP-IPC
title: "Contrato del canal local (JSON-RPC 2.0)"
type: api-spec
status: draft
domain: GRP
feature: motor-local
created: 2026-10-04
updated: 2026-10-06
related:
  adrs: [ADR-GRP-005, ADR-GRP-016, ADR-GRP-011, ADR-GRP-013, ADR-CKP-002, ADR-CKP-003, ADR-TMC-004]
  stories: [TS-GRP-004, US-GRP-001, US-GRP-012, TS-TMC-004, TS-CKP-002, INF-CKP-001]
  specs: [DS-TS-GRP-004, DS-US-GRP-001, DS-US-GRP-012, DS-TS-TMC-004, DS-TS-CKP-002]
  deps: [DEP-CKP-6]
tags: [ipc, json-rpc, contrato, eventos, mcp, seguridad]
---

# Contrato del canal local

La fuente de verdad es el código de `crates/api`. Este documento es su resumen para quien consume el canal: CLI/TUI, `raptor-mcp`, el Cockpit y las historias que añaden métodos. Lo fija [ADR-GRP-005](../decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 5 y lo implementa [TS-GRP-004](../../requirements/features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md). La descripción OpenRPC que cita el overview queda pendiente; `schemars` ya genera el JSON Schema de cada tipo.

## Transporte y marco

- Socket Unix `raptor.sock` en la carpeta de ejecución del perfil, con la carpeta en 0700 y el socket en 0600. Solo acepta clientes del mismo uid. El cliente comprueba que el servidor también es de su uid. En Windows, named pipe: pendiente.
- Un mensaje JSON por línea (`\n`), de 1 MiB como máximo y con una profundidad máxima de 32. No se aceptan batches. Todos los tipos rechazan campos desconocidos.
- `PROTOCOL_VERSION = 9`, **congelado para cambios aditivos** (`API_VERSION` 9.0.0, ADR-GRP-016 § 1). La 7 añade `guard.*`, el código `-32016` y el rechazo `not-observed` (US-GRD-001); la 8, el tipo de evento `reset` (US-TMC-004); la 9, las capacidades (`hello.capabilities` y `connection.accept`). Desde la 9, un método nuevo se descubre en `hello.methods` y un cambio de forma es una capacidad: el número ya no sube. El daemon atiende a clientes de `MIN_COMPATIBLE_PROTOCOL = 5` a `9`, cada uno con las formas de su versión. `hello` responde con el protocolo negociado. Un cliente más nuevo que el daemon lo reemplaza, y uno más viejo que la ventana recibe `-32002`. Hasta la 5, la compatibilidad era por igualdad.
  - La 2 (US-GRP-001) añade el estado de los worktrees a `RepoView`.
  - La 3 (US-GRP-012), la rama base del repo y el ahead/behind de cada worktree.
  - La 3.1.0 (US-GRP-002) es aditiva: `events.history` y el `data` de `git.event`; un cliente nuevo comprueba en `hello.methods` que el daemon ofrece `events.history` y, si no, pide reiniciarlo.
  - La 4 (TS-CKP-002), el catálogo de operaciones en dos fases: `operation.run` ejecuta un plan preparado. Incluye todo lo de la 3.1.0.
  - La 5 (US-GRP-007): sesiones de agente, `session.state` y `sessions.list`.
  - La 6 (TS-GRP-004, N1 a N7 de ADR-CKP-003 § 4) añade, para el Cockpit:
    - ámbitos con secuencia propia (`scope.snapshot`, `scope.subscribe`, `scope.event`, `scope.resync`);
    - `repo.locate`;
    - `requester` en `hello`;
    - nombres no confiables acotados;
    - códigos tipados.

    Una conexión 5 no ve nada de esto: no le llegan los métodos nuevos ni `requester`, y el resto del cable no cambia.
  - La 6.1.0 (US-GRP-017) es aditiva: `engine.resources`. La CLI comprueba en `hello.methods` que el daemon lo ofrece y, si no, pide reiniciarlo.
  - La 6.2.0 (US-GRP-009) es aditiva: `registration.register`, `registration.withdraw` implementado, el código `-32015` y los motivos de auditoría `worktree-mismatch` y `agent-mismatch`.

## Handshake

El primer mensaje es `hello`; si no llega en 2 s, el daemon cierra la conexión. La forma de `hello`, `IncompatibleData` y `daemon.replace` **no cambia entre versiones**.

```json
{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":2,"client":"cli","client_version":"0.0.0"}}
```

El resultado trae `protocol`, `binary_version`, `instance_id` (del perfil, ADR-GRP-006 § 4), `daemon_pid`, `profile` (`full` o `mcp`), `max_message_bytes` y `methods`: los métodos que puede llamar esa conexión. Si la versión no coincide, el daemon responde `-32002` con `{daemon_protocol, binary_version}`. Si el cliente es más nuevo, puede enviar `daemon.replace`.

### Capacidades (protocolo 9, ADR-GRP-016 § 1)

Una conexión de protocolo 9 recibe también `capabilities`: los nombres de todas las capacidades que sirve el daemon. Las conexiones de 5 a 8 no lo reciben, porque rechazan campos desconocidos. Una capacidad es un **cambio de forma**, se llama `<módulo>.<feature>` y la declara su módulo en `crates/api/src/methods/`.

| Capacidad | Legada de | Qué cambia |
|---|---|---|
| `connection.requester` | protocolo 6 | `hello.requester` |
| `events.git-reset` | protocolo 8 | Eventos Git de tipo `reset` en el stream y en `events.history` |

Una conexión tiene las capacidades legadas de su protocolo (todas, si es de protocolo 9) y las que acepte con `connection.accept`. Si no llama a ese método, recibe las formas del protocolo 8. Desde el protocolo 9, `daemon.replace` con el **mismo** protocolo lo acepta el daemon solo si viene del binario instalado y actualizado; si no, responde `-32602` y la conexión sigue. El cliente lo pide cuando el daemon no anuncia una capacidad que él conoce.

## Métodos

| Método | Reservado | MCP | Resultado / notas |
|---|---|---|---|
| `ping` | No | Sí | `"pong"` |
| `connection.accept` | No | Sí | Protocolo 9. `{capabilities: [nombre]}` (como mucho 64, de 64 caracteres) → `{capabilities}`: las que tiene ahora la conexión. Una sola vez y antes de la primera suscripción; si no, `-32600`. Ignora los nombres que el daemon no sirve |
| `engine.snapshot` | No | Sí | `Snapshot { run_id, seq, engine, daemon, repos }`, cada repo con sus `worktrees` (ver más abajo); en MCP, `McpSnapshot { run_id, seq, engine_state, caller_repo }` (allowlist de campos) |
| `engine.resources` | No | No | `{}` → `ResourcesResult { process {cpu {mean_pct?, peak_pct?, window_s}, rss_bytes?, open_fds?}, watches {roots, inotify? {watches, max_user_watches?}}, disk {profile_bytes, time_machine [{repo_id, bytes}], complete}, pools?, power_saving?, targets }`. Solo números, booleanos y enums, sin texto de presentación (NFR-10); `null` = no disponible. CPU en % de un núcleo sobre una ventana de hasta 10 min; RSS y descriptores instantáneos; disco en bytes asignados. `pools` (TS-GRP-005) y `power_saving` (US-GRP-019) llegan `null` hasta sus historias. Fuera del MCP (SEC-MCP-01) (US-GRP-017) |
| `events.subscribe` | No | Sí | `{ from_seq?, run_id? }` → `{ subscription, from_seq }`. Hasta 4 por conexión |
| `events.unsubscribe` | No | Sí | `{ subscription }` → `bool` |
| `audit.list` | No | No | `{ after_id?, limit? }` → `{ entries: [AuditEntry] }`. Como máximo 500 por página |
| `events.history` | No | No | `{ repo_id, worktree?, after_seq?, limit? }` → `{ events: [GitEventView] }`, del más antiguo al más reciente. Sin `after_seq`, los `limit` más recientes. Como máximo 200 por página. Fuera del MCP porque lleva rutas (SEC-12) (US-GRP-002) |
| `daemon.stop` | **Sí** | No | `{ stopping: true }`; luego el daemon cierra las conexiones |
| `daemon.replace` | Solo si no viene del binario instalado | Sí | `{ protocol }`: más nuevo que el del daemon o, desde el 9, el mismo si viene del binario instalado y actualizado |
| `repo.add` | **Sí** | No | `{ path }` (raíz de un worktree o directorio Git, sin búsqueda hacia arriba) → `{ outcome: new\|already-observed\|reactivated, repo: RepoView }`. Autoriza y audita **antes** de leer la ruta (US-GRP-001) |
| `repo.retire` | **Sí** | No | `{ repo_id }` → `{ retired }`. Deja de observar y conserva los datos (US-GRP-001; US-GRP-006 añade el historial y el hueco) |
| `attribution.correct`, `attribution.withdraw-correction` | **Sí** | No | Declarados. Los implementa US-GRP-010 |
| `registration.register` | No | Sí | `{ agent: {kind:"claude-code"}\|{kind:"other", name}, worktree? }` → `{ repo_id, session_id, outcome: created\|confirmed\|already-registered, actor, support: full\|observed }`, sin rutas (SEC-12). Quien pasa los controles 1 a 3 es el desarrollador y nombra el worktree; cualquier otro, y toda conexión MCP, es un agente que se registra en el worktree de su cwd y como el agente que es (ADR-GRP-005 § 6.6, Enmienda 2026-10-05). Los rechazos `worktree-mismatch` y `agent-mismatch` se auditan (US-GRP-009) |
| `registration.withdraw` | **Sí** | No | `{ worktree, agent }` → `{ repo_id, session_id }`. Termina la sesión creada por el registro de ese agente (`registration-withdrawn`); una sesión detectada, aunque esté confirmada, termina con su proceso (US-GRP-009) |
| `requester.resolve` | No | Sí | Cómo ve el daemon al llamante (ADR-TMC-005 § 1). En MCP, solo `{actor, channel}` |
| `operation.describe` | No | Sí | Catálogo de operaciones (ADR-CKP-002 § 1): `{catalog_version, operations: [{id, class, governed, cockpit, mcp, args_schema}]}`. En MCP, solo las operaciones con marca MCP y sus esquemas MCP (`create-worktree` sin `path`) |
| `operation.prepare` | No | Sí | Primera fase, sin efectos ni oplog. `{operation, worktree?, args, surface?, session_env?}` → `{plan_id, catalog_version, operation, layer, requester, fingerprint, warnings, decision, challenge?, expires_in_ms, diagnostics}`. En MCP, el worktree sale del cwd del llamante y la respuesta es la proyección sin `requester`, `layer`, `challenge` ni `diagnostics`. Una operación cuya historia no existe todavía responde `-32004` con `implemented_by`; un quinto plan vivo en la conexión, `-32006` |
| `operation.run` | No | Sí | Segunda fase: ejecuta un plan **de la misma conexión** como operación protegida. `{plan_id, accepted_warnings, confirmation?}` → `{operation_id, prior_snapshot_id, fast_path, requester, changed_refs, outcome, layer, git_output?}`. `git_output` (texto no confiable) solo va a la capa `cockpit`. En MCP: `{operation_id, prior_snapshot_id, actor, changed_refs, outcome}`. El plan se consume en cualquier intento |
| `operation.cancel` | No | No | `{operation_id}` → `{requested}`. Exige capa `cockpit`; un descendiente del ejecutor nunca la tiene |
| `scope.snapshot` | No | No | Protocolo 6. `{scope}` → `{"scope":"global", run_id, scope_seq, engine, daemon, autostart, repos: [{repo_id, state, path, attention}]}` o `{"scope":"repo", run_id, scope_seq, repo: RepoView}`. `attention = {conflicts, denials, gaps}`, cada uno `{"state":"counted","count":n}` o `{"state":"unavailable","reason":"not-published"}` (hoy, los tres sin publicar). `autostart`: `registered`, `not-registered` o `unknown` (hoy `unknown`, US-GRP-004). Un repo no observado responde `-32009` |
| `scope.subscribe` | No | No | Protocolo 6. `{scope, from_seq?, run_id?}` → `{subscription, scope, from_seq}`. `from_seq` es la secuencia del ámbito. Cuenta en el límite de 4 suscripciones y comparte ids con `events.subscribe`; se cancela con `events.unsubscribe` |
| `repo.locate` | No | No | Protocolo 6. `{path}` → `{repo_id, worktree}`: el repo observado y la raíz del worktree que contienen la ruta (la más profunda). Validación léxica antes de tocar el FS (`-32602` con `data.reason`). Fuera de los repos observados o inexistente, `-32009` |
| `guard.plan`, `guard.status` | No | No | Protocolo 7 (US-GRD-001). `{path}` → qué instalaría la capa de hooks y por qué no (`GuardPlan`, con `status`, `blockers`, lo no impedible y la rama base) o el estado de protección (`GuardStatus`: `unprotected`/`hooks-only`, permiso, `offer`, ramas base protegidas, último rechazo). Repo no observado: `-32013` con `not-observed` |
| `guard.install`, `guard.decline` | Sí | No | Protocolo 7. `{path}` → `GuardStatus`. `guard.install` escribe `core.hooksPath` y `<común>/gitraptor/` (`RepoWrite::Guardrails`, recuperable por su diario); si no se puede instalar, `-32016` con `data.blockers`. `guard.decline` guarda la denegación del permiso |
| `guard.evaluate` | No | No | Protocolo 7. Lo llama `raptor hook` con las constantes de su dispatcher: `{repo_id, common_dir, hook, operation}` → `Decision` (`decisionId`, `effect`, `appliedEffect`, `reasons[]`, `exception`, `configStatus`, `configRef`; ADR-GRD-003 § 3). Se atiende en el hilo de la conexión, sin esperar al bucle ni al cerrojo del repo |
| `timemachine.*` | No | Según el método | Declarados con sus parámetros (TS-TMC-004); los implementan US-TMC-002, 003, 005, 006 y 009 |

La **capa** (`cockpit` o `mcp`) la fija el daemon según el solicitante resuelto, nunca el cliente. Con capa `mcp` solo se ofrecen las operaciones con marca MCP, y un "sin atribuir" sin capa `cockpit` no recibe ninguna (ADR-CKP-002 § 4, M-03).

Un comando reservado lo decide **solo el daemon**, con la identidad del par y sin fiarse de nada que declare el cliente: ascendencia sin agentes y sin el propio daemon, líder de sesión y terminal de control (ADR-GRP-005 § 6 y su Enmienda TS-GRP-004). Cada intento, aceptado o no, queda en la auditoría append-only y se publica como evento `reserved.audit`. Un método declarado responde `-32004` con `{implemented_by}` después de autorizar y auditar.

La conexión de protocolo 6 de un cliente `cli` con perfil completo recibe en `hello` el campo `requester`. Vale `{"state":"resolved", actor, layer, confirmable}` o `{"state":"unverified"}` (N5) y es solo UX: el daemon resuelve otra vez en cada petición, y tras un `-32012` el cliente vuelve a llamar a `requester.resolve`.

## Errores

Un cliente presenta cada error por su `code` y su `data`, nunca por `message`, que queda para el log (N7). La lista de abajo está **congelada en `-32016`** (`gitraptor_api::rpc::ErrorCode`). Un código nuevo lo declara su módulo en su bloque de 20, desde `-33000` hacia abajo (`Group::error_block` y `ErrorSpec`, ADR-GRP-016 § 3), y `rpc::error_name` da el nombre estable de cualquier código. Desde la 6, `-32602` de los validadores de rutas y nombres lleva `data.reason` (`InvalidReason`: `empty`, `too-long`, `not-absolute`, `control-character`, `unc-or-device`, `device-name`, `alternate-stream`, `outside-observed`, `invalid-ref`, `reserved-name`), y `-32010` también (`ScopeRefusal`: `no-working-folder`, `not-observed`, `not-allowlisted`, `unattributed-over-mcp`, `foreign-worktree`). Es aditivo: un cliente 5 lo ignora.

| Código | Significado |
|---|---|
| `-32700`, `-32600`, `-32601`, `-32602`, `-32603` | JSON-RPC estándar (análisis, petición, método, parámetros, interno) |
| `-32001` | Falta el `hello` (la conexión se cierra) |
| `-32002` | Versión de protocolo incompatible |
| `-32003` | Comando reservado rechazado; `data.reason`: `agent-ancestry`, `session-leader-agent`, `daemon-descendant`, `no-controlling-terminal`, `identity-unverified`, `not-available-to-mcp` o `unsupported`. En la auditoría, además, `worktree-mismatch` y `agent-mismatch` (rechazos de `registration.register`, que no es reservado) |
| `-32004` | Declarado, pero sin implementar todavía |
| `-32005` | Rate limit (100/s, ráfaga de 200). La conexión sigue abierta |
| `-32006` | Límite de conexiones o de suscripciones |
| `-32007` | No se puede continuar la suscripción: hay que tomar una instantánea nueva |
| `-32008` | Falló el snapshot previo: la operación no se ejecutó y el repo no cambió; `data`: `{reason, operation_id?}` |
| `-32009` | No existe para este llamante (un id de otro repo responde igual que uno desconocido) |
| `-32010` | Ámbito rechazado (sin carpeta de trabajo legible, fuera de un repo observado o fuera de la allowlist MCP) |
| `-32011` | La operación empezó tras su snapshot previo y falló: queda `interrupted` y se puede deshacer |
| `-32012` | La identidad del llamante cambió desde que se aceptó la conexión |
| `-32013` | Repo rechazado por lo que nombra (no por quién lo pide); `data.reason`: `not-a-repo`, `untrusted`, `unreadable`, `unknown-repo` o, desde el protocolo 7 y solo en `guard.*`, `not-observed`. (Hasta 2026-10-05 este documento lo listaba por error como `-32008`) |
| `-32014` | Plan del catálogo rechazado antes de ejecutar nada, sin apunte en el oplog (TS-CKP-002). `data.reason`: `state-changed`, `plan-unknown`, `not-available-for-layer`, `unattributed-without-cockpit`, `executor-descendant`, `warnings-mismatch`, `confirmation-required`, `challenge-invalid`, `foreign-work`, `other-session-present`, `operation-in-progress`, `detached-head`, `git-busy`, `worktree-locked`, `branch-checked-out-elsewhere`, `grafts`, `repo-identity-changed`, `guardrails-denied`, `new-path-refused`, `queue-full` o `daemon-stopping` |
| `-32015` | Registro de agente o su retiro rechazado (US-GRP-009). `data.reason`: `not-a-worktree`, `repo-not-observed`, `worktree-mismatch`, `agent-mismatch`, `no-working-folder` o `not-registered` |
| `-32016` | Protocolo 7 (US-GRD-001): no se instaló la capa de hooks; `data.blockers`: `prior-hooks`, `worktree-config`, `include-defines-hooks-path`, `include-if-onbranch`, `not-representable`, `orphan-folder`, `already-installed`, `dispatcher-missing`, `bare` o `platform-unsupported` |

## Eventos

Notificación `events.event` con `{ subscription, event }`. El evento lleva:

```json
{"seq":42,"kind":"git.event","version":1,"wall_ms":1791148066018,
 "timings":{"batch_id":7,"t_recv":…,"t_flush":…,"t_computed":…,"t_persisted":…,"t_published":…},
 "data":{…}}
```

- `seq` es único en el daemon y estrictamente creciente. Vuelve a empezar en cada arranque, y por eso existe `run_id`.
- Los eventos de cambio llevan siempre `timings`, en nanosegundos del reloj monótono común (`gitraptor_api::clock::monotonic_ns`, ADR-GRP-011 § 3). El cliente añade `t_client_recv` con el mismo reloj.
- Tipos propios del motor (sin `timings`): `engine.state`, el primer evento de cada ejecución y cada transición de BR-WF-002; `daemon.stopping`; `reserved.audit`; y `repo.observation` `{repo_id, observed, state, path}` al añadir o retirar un repo (US-GRP-001).
- `operation.queued` `{repo_id, operation, layer, position}`, `operation.started` `{…, operation_id}` y `operation.finished` `{…, operation_id?, outcome}` (TS-CKP-002, Q-CKP-19). Sin argumentos ni salida de Git.
- `worktree.state` (US-GRP-001, de cambio): `{repo_id, worktrees}` con todos los worktrees del repo reconciliado.
- A `raptor-mcp` solo le llegan `engine.state` y `daemon.stopping` (allowlist, SEC-12); el resto lleva rutas o auditoría.
- `git.event` (US-GRP-002, de cambio): un `GitEventView` por evento de Git, ya persistido en el historial del repo:

```json
{"repo_id":"…","seq":12,"worktree":{"untrusted":"/w/demo-feat"},"kind":"commit",
 "actor":{"actor":"unattributed"},"observed_utc_ms":1791148066018,"utc_offset_s":7200,
 "details":{"branch":{"untrusted":"feat-login"},"old_commit":"…","new_commit":"…","worktree_inferred":false}}
```

  `kind`: `commit`, `merge`, `rebase`, `branch-update`, `branch-create`, `branch-delete`, `branch-switch`, `worktree-create`, `worktree-delete`, `push`, `reconciled` (diferencias que encontró una reconciliación, con `gap_id`; no es un comando de Git) o, desde el protocolo 8, `reset` (un `reset` que no mueve rama, o con `HEAD` separado, US-TMC-004; una conexión de protocolo 5 a 7 no lo recibe ni en `events.history`). `seq` es la secuencia del historial del repo (ADR-GRP-013 § 4), no la del stream. `worktree_inferred` dice que Git no indica desde qué worktree se ejecutó el comando y el motor lo dedujo. Sin sesión con evidencia, el actor es `unattributed` (BR-CONS-003). El observador publica además `worktree.state` en cuanto cambia un worktree, con los `timings` de su ventana de debounce.
- Tipos de cambio declarados, con `data` definido por su historia: `gap.recorded` (US-GRP-005), `session.state` (US-GRP-007) y `attribution.changed` (US-GRP-010).
- **Arranque coherente (DEP-CKP-6)**: `engine.snapshot` devuelve `seq = N`. Después, `events.subscribe { from_seq: N + 1, run_id }` no pierde ni repite eventos: el daemon guarda los últimos 1024. Si el `run_id` ya no es el del daemon o `N + 1` salió del buffer, llega `events.resync` con su `reason` (`daemon-restarted` o `replay-unavailable`).
- **Cliente lento**: si su cola de 1024 mensajes se llena, recibe `events.resync { reason: slow-consumer }` y se le desconecta. Nunca frena al productor.
- **Ámbitos (protocolo 6, N1 y N2)**: cada evento pertenece a un ámbito, `{"scope":"global"}` o `{"scope":"repo","repo_id":…}`.
  - **Globales**: `engine.state`, `daemon.stopping`, `reserved.audit`, `repo.observation` y `repo.attention` (`{repo_id, attention}`, declarado sin emisor todavía).
  - **Del repo**: `worktree.state`, `git.event`, `operation.*` y los tipos declarados por historias.
  - Cada ámbito tiene su secuencia contigua (`scope_seq`), asignada bajo el mismo lock que la global.
  - **Arranque coherente**: `scope.snapshot` en `N` y `scope.subscribe { from_seq: N + 1, run_id }`. Llegan notificaciones `scope.event { subscription, scope, scope_seq, event }`, y la del ámbito anterior siempre tiene `scope_seq - 1`. Un salto es un hueco: pide otra instantánea.
  - **`scope.resync { scope, reason }`**: llega si `N + 1` salió del buffer (`replay-unavailable`), si el daemon se reinició (`daemon-restarted`) o si el repo dejó de observarse (`scope-closed`; la suscripción termina y la secuencia del repo no se reinicia).
  - El cliente lento sigue siendo de conexión: `events.resync { slow-consumer }` vale para todos sus ámbitos.

## Estado de un worktree (US-GRP-001)

```json
{"path":{"untrusted":"/w/demo-feat"},"main":false,"admin_name":{"untrusted":"demo-feat"},
 "status":{"state":"ready","head":{"kind":"branch","name":{"untrusted":"feat-login"}},
           "counts":{"staged":0,"unstaged":1,"untracked":0},
           "changes":[{"path":{"untrusted":"login.txt"},"area":"unstaged","kind":"modified"}]}}
```

- `head`: `branch {name}`, `unborn {name}` (rama sin commits) o `detached`. `status` también puede ser `{"state":"unavailable","reason":"missing"|"untrusted"|"unreadable"}`; la semántica de los estados especiales es de US-GRP-003.
- Limpio = todos los `counts` a cero. `changes` está ordenado y acotado a 200 rutas y 32 KiB por worktree. Si un mensaje pasa de 768 KiB, se vacían las listas y se conservan los conteos.
- Lo recalcula una reconciliación completa al añadir el repo y al arrancar el motor, y después el observador de US-GRP-002 en cada cambio.

## Rama base y ahead/behind (US-GRP-012)

```json
"base":{"name":{"untrusted":"main"},"status":"unconfirmed"}
"divergence":{"state":"counted","ahead":{"count":3,"exact":true},"behind":{"count":1,"exact":true}}
```

- `RepoView.base`: la rama base confirmada del almacén por repo (`confirmed`) o, sin confirmación, `main` (`unconfirmed`). `invalid` (sin `name`) solo llega con la configuración del equipo de US-GRP-016. Añadir el repo nunca confirma nada (ADR-GRD-004 § 3.5).
- `divergence` va dentro de `status` `ready`: `counted {ahead, behind}`, cada lado `{count, exact}` (`exact: false` si el recorrido llegó al tope de 10 000); `base-missing` (no existe `refs/heads/<base>`; no se usa ninguna otra ref, Q42); `no-base`; `no-commits`; o `unreadable`.
- Se calcula en la reconciliación y otra vez en cada `engine.snapshot` de una conexión completa, con la punta de la base leída en ese momento y la rama de cada fila (DS-US-GRP-012 D5). Un cambio observado (US-GRP-002) se publica en dos fases: primero un `worktree.state` con el estado nuevo y el ahead/behind anterior, y después otro con el ahead/behind contado de nuevo si cambió (ADR-GRP-010 § 4).

## Texto no confiable y actor

- Todo texto que viene del repo o de un agente viaja como `{"untrusted": "…"}`, con `truncated` y `lossy` cuando aplican. Cada clase de campo tiene su tipo y su tope (N6): `Untrusted` (rutas y texto, 4096 bytes) y `UntrustedName` (ramas, nombres de worktree y de agente, 1024 bytes). El JSON Schema lleva `maxLength`, y el decodificador impone el tope (recorta y marca `truncated`) también en el cliente. Los clientes muestran solo `Untrusted::sanitized()`, que quita escapes ANSI, OSC y DCS, controles, bidi y caracteres de ancho cero (SEC-12).
- El actor es `{"actor":"agent","kind":"claude-code"|"other","name"?,"origin":"detected"|"registered"}` o `{"actor":"unattributed"}`. No existe la variante "humano" (ADR-GRP-013 § 6).

## Pendiente

- Descripción OpenRPC generada desde los tipos.
- Windows (named pipe).
- Consultas bajo demanda del Cockpit (N8; DEP-CKP-2 y DEP-CKP-3, enmienda de ADR-GRP-005 § 5), preferencias de la TUI (N9) y resolución del editor (N10).
- Emisores de `repo.attention` (predictor TS-CKP-001, Guardrails, US-GRP-005). El `autostart` de `scope.snapshot` ya es real desde US-GRP-004: `registered` o `not-registered` según exista el artefacto de `raptor daemon enable`, y `unknown` solo si el SO no tiene autoarranque compatible.
- Timeline (DEP-CKP-5). La última actividad (DEP-CKP-4) y la antigüedad del fetch ya viajan con la capacidad `scope.activity` (2026-10-06, DS-US-CKP-001 § 6), derivadas en memoria. Falta derivarlas de los eventos del almacén al arrancar, con la marca del hueco.
- `caller_repo` real en macOS (F-001-05): hoy `process_cwd` devuelve `None` en macOS; lo implementa US-MCP-003 con la doble comprobación de identidad de ADR-MCP-001 § 2.
- Servidor MCP (ADR-MCP-001, 2026-10-05), cada método lo añade su historia dueña: comandos reservados `mcp.enable` y `mcp.disable` (US-MCP-002); perfil `mcp` por solicitante agente, sea cual sea el cliente (US-MCP-003); rate limit de lecturas por conexión `mcp`, 120 por minuto con ráfaga de 30, con el `RATE_LIMITED` congelado (US-MCP-005; los límites por solicitante, con US-MCP-008/009); capacidad `mcp.status-branch`: `mcp.status` añade `branch` (texto no confiable, ausente con `HEAD` separado) (US-MCP-005); vista MCP ampliada de `engine.snapshot` (US-MCP-004); registro y retiro del propio agente, no reservados (US-MCP-006); vista MCP de `timemachine.timeline` (US-MCP-017); consulta de la predicción para el perfil `mcp` (US-MCP-016). El contrato de ejecución quedó unificado por TS-CKP-002 (protocolo 4).
- Catálogo de producción del ejecutor: hoy `operation.prepare` responde `-32004` (`F-001-02`) mientras el daemon no lleve un catálogo; lo cablea US-MCP-008 (DS-TS-CKP-002 § 9).


> **Compatibilidad del protocolo 7** (US-GRD-001; Decisión del orquestador, 2026-10-06): una conexión de protocolo 5 o 6 no ve `guard.*` en `hello` y, si los pide, recibe `-32601` aunque sean reservados (no se auditan); `-32016` y `not-observed` solo salen de `guard.*`, así que sus formas no cambian.

> **Compatibilidad del protocolo 8** (US-TMC-004; Decisión del orquestador, 2026-10-06, validada por el Arquitecto): el único cambio es el tipo de evento `reset`. Una conexión de protocolo 5 a 7 no lo sabría leer, así que el daemon no se lo entrega ni por suscripción (`events.event`, `scope.event`) ni en `events.history`; el resto de sus formas no cambia.
