---
id: DS-TS-GRP-003
title: "Dev Spec — Proceso del motor en segundo plano por usuario"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-04
updated: 2026-10-04
related:
  stories: [TS-GRP-003]
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-013]
  nfrs: [NFR-01, NFR-03, SEC-05, SEC-06, SEC-10, SEC-13]
tags: [motor-local, daemon, ciclo-de-vida, instancia-unica, parada-ordenada, logs, seguridad]
---

# Dev Spec — TS-GRP-003: Proceso del motor en segundo plano por usuario

Plano de ejecución compacto (AADD ligero) de [TS-GRP-003](../technical-stories/TS-GRP-003-proceso-motor.md). La forma del motor la fija [ADR-GRP-005](../../../../architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 1, § 2 y § 4, y el perfil [ADR-GRP-006](../../../../architecture/decisions/ADR-GRP-006-perfil-ubicacion-almacenamiento.md). Esta spec solo dice **cómo** se construye en `crates/core::daemon` y en el subcomando `raptor daemon` de `apps/cli`. Reutiliza el almacén del perfil (`crates/core::profile`, TS-GRP-001) y la resolución de Git (`crates/git::resolve`, TS-GRP-002).

## 1. Decisiones previas a la spec

Todas son **Decisión del orquestador (2026-10-04), validada por Arquitecto**. El Arquitecto revisó las nueve propuestas y pidió los ajustes que ya están incorporados abajo.

| # | Decisión | Ajuste del Arquitecto |
|---|---|---|
| D1 | Bloqueo con `std::fs::File::try_lock` (std ≥ 1.89: `flock` / `LockFileEx`), sin crate nuevo | Verificar la carpeta de estado antes del lock; no fiarse del PID sino del lock; prueba de que un hijo no hereda el lock (CLOEXEC); el código de salida 3 debe excluirse del relanzamiento en US-GRP-004 |
| D2 | Marca de ejecución en `profile_meta` del índice global, sin migración | **Una señal no identifica a quien la envía**: una parada por señal con sesión activa se clasifica como `daemon-down-during-session`; `daemon-stopped` queda solo para el comando de parada autorizado (TS-GRP-004) |
| D3 | Parada ordenada por `ShutdownHandle`; señales Unix con `signal-hook` | SIGHUP no es cierre de sesión (launchd y systemd mandan SIGTERM); plazo interno de 5 s; orden: "observado hasta" → marca de parada → log → lock |
| D4 | `raptor daemon stop` provisional por señal, sin canal | Comprobar el lock justo antes de enviar la señal; el rechazo de `stop` desde un agente (SEC-13) se prueba en TS-GRP-004 |
| D5 | Máquina de estados como tipos | Sin Git manda "Esperando Git" aunque haya repos; "Observando" nunca con cero repos |
| D6 | Entorno capturado una vez por allowlist (`DaemonEnv`) | Ignorar `XDG_*_HOME` en **todos** los procesos (una sola función de resolución), con enmienda a ADR-GRP-006 § 1; prueba estática contra lecturas sueltas del entorno |
| D7 | Logs con redacción por tipos | En el panic hook, solo el nombre del archivo (la ruta puede llevar la carpeta personal de la máquina de build); stderr igual de redactado |
| D8 | `DaemonConfig` inyectable (perfil, entorno, candidatos de Git, latido, límites del log, plazo de parada) | `ShutdownHandle` también inyectable |
| D9 | Gate de seguridad de `non-functional.md` | Se da por levantado para esta spec porque Rene Bonilla aceptó ADR-GRP-005 el 2026-10-04, con el mismo criterio que TS-GRP-002 (romper el ciclo con INF-GRP-001). ⚠️ **ASSUMPTION**: al aceptar el ADR, Rene eximió la condición "INF-GRP-001 con auditoría dinámica de `exec`", que sigue abierta en INF-GRP-001. El texto del gate en `non-functional.md` sigue diciendo "bloqueadas": queda pendiente actualizarlo (fuera de esta rama) |

## 2. Dependencias

| Crate | Versión | Dónde | Por qué |
|---|---|---|---|
| `signal-hook` | `0.4.5` (+ `signal-hook-registry 1.4.8`) | `crates/core`, solo `cfg(unix)` | Convertir SIGTERM, SIGINT y SIGHUP en peticiones de parada sin `unsafe` propio (`unsafe_code = "forbid"`). MIT o Apache-2.0 |

Nada más. El bloqueo usa `std`, la señal de `daemon stop` usa `rustix` (ya en el árbol) y el log se escribe a mano.

## 3. Estructura

| Módulo | Responsabilidad |
|---|---|
| `daemon/mod.rs` | `Daemon` (arranque, bucle, parada), `DaemonConfig`, `StartupReport`, `classify_previous_run`, `run_process` (punto de entrada) |
| `daemon/lock.rs` | `InstanceLock` (`state/daemon.lock`), `running_pid`, `wait_until_released` |
| `daemon/state.rs` | `EngineState` y `Trigger`: las transiciones de BR-WF-002 como función pura |
| `daemon/env.rs` | `DaemonEnv`: entorno por allowlist, `Invoker` y `ResolveConfig` construidos desde él |
| `daemon/log.rs` | `Logger` con rotación y `Field` cerrado; panic hook que redacta |
| `daemon/shutdown.rs` | `ShutdownHandle`, `StopCause`, señales y `signal_running_daemon` |
| `profile` (TS-GRP-001) | Añade `Profile::daemon_run`, `mark_daemon_running` y `mark_daemon_stopped` (claves de `profile_meta`) y `RepoStore::has_active_sessions` |
| `apps/cli` | `raptor daemon` (ejecuta el motor en primer plano) y `raptor daemon stop` |

## 4. Ciclo de vida

**Arranque** (`Daemon::start`), en este orden:

1. `umask 077`. Crea o verifica, con 0700 y propietario, solo las carpetas que llevan a `state/`.
2. **Bloqueo**: abre `state/daemon.lock` (0600, `O_NOFOLLOW`, archivo normal del mismo uid, sin arreglarlo con `chmod`) y `try_lock`. Si está tomado, devuelve `AlreadyRunning`: el segundo daemon **no abre el log, el índice ni los almacenes**, escribe solo en stderr y sale con código **3** (`EXIT_ALREADY_RUNNING`). Con el lock tomado escribe su PID.
3. Abre el log y el perfil (`Profile::open`, que verifica permisos y migra).
4. Lee la marca de la ejecución anterior **antes** de escribir `running`.
5. Resuelve Git con `DaemonEnv::invoker()` y `git_resolve_config()`. Con PATH vacío, sirven las rutas conocidas del SO.
6. Por cada repo observado abre su almacén y calcula su `PendingGap`, que contiene el "observado hasta" anterior, la causa y el solicitante. Un almacén que no abre (esquema más nuevo) se anota en `unavailable` y no se observa.
7. Estado inicial `EngineState::initial(git, repos)`. Fuera de "Observando" se sueltan los almacenes.

**Clasificación de la ejecución anterior** (`classify_previous_run`, ADR-GRP-013 § 5, SEC-13):

| Marca anterior | Sin sesión activa | Con sesión activa |
|---|---|---|
| Ninguna (primer arranque) | — | — |
| `running` (no registró parada: caída o `kill -9`) | `daemon-down` | `daemon-down-during-session` |
| `stopped` por señal | `daemon-down` (`requested_by = signal:TERM`) | `daemon-down-during-session` (`signal:TERM`) |
| `stopped` por comando autorizado (TS-GRP-004) | `daemon-stopped` con el cliente | `daemon-stopped` con el cliente |

El daemon **calcula y expone** la clasificación en `StartupReport::repos` y en el log (`repo_pending_gap`), pero **no abre el hueco**: registrarlo es de US-GRP-005, que debe hacerlo antes del primer latido, porque el latido mueve "observado hasta".

**En marcha** (`Daemon::run`): espera peticiones en el `ShutdownHandle`. Mientras observa, persiste "observado hasta" cada 60 s en cada almacén (latido configurable).

**Parada ordenada** (`Daemon::stop`), ante SIGTERM, SIGINT, SIGHUP, `raptor daemon stop` o, más adelante, el comando del canal:

1. Arranca un vigilante de 5 s: si la parada se atasca, el proceso sale y el siguiente arranque lo ve como caída.
2. `SetObservedUntil(ahora)` en cada almacén.
3. Solo si todas las marcas se escribieron: `mark_daemon_stopped(ahora, causa, solicitante)`. Si alguna falló, la parada no se registra como ordenada.
4. Registra `daemon_stopped` en el log y lo vuelca a disco.
5. Vacía el PID del lock y lo suelta.

**Proceso** (`run_process`): instala el panic hook que redacta, fija la carpeta de trabajo en `state/` y las señales, y ejecuta el bucle.

**`raptor daemon stop` provisional** (D4): lee el PID con `running_pid`, que comprueba con un lock compartido que el lock sigue tomado justo antes de leerlo, envía SIGTERM y espera hasta 10 s a que el lock se libere. Equivale a un `kill` que cualquier proceso del usuario ya puede hacer, así que no rebaja SEC-13. La parada se registra "por señal" y nunca como parada atribuida. En Windows devuelve "no soportado" hasta que exista el canal.

## 5. Seguridad

- **SEC-05 (logs)**: `state/daemon.log`, 0600, línea `<utc_ms> <NIVEL> <evento> clave=valor…`. El evento es `&'static str` y los valores son `Field`, un tipo cerrado: números, booleanos, texto estático, versión de Git ya parseada, ids opacos (solo hex y guiones) y la ubicación de un pánico (solo el nombre del archivo y la línea). **No se puede construir** un campo con una ruta de repo, contenido, configuración, entorno ni argv. Los errores se registran por tipo, nunca con su mensaje. Rotación por tamaño: 1 MiB y 3 archivos. El panic hook registra solo dónde ocurrió el pánico y escribe en stderr un texto fijo.
- **SEC-06**: carpeta de estado, lock y logs privados. Una carpeta 0755 o un lock 0644 impiden arrancar, sin `chmod`.
- **SEC-10 (entorno)**:
  - `DaemonEnv::capture()` se llama una vez y conserva, por allowlist, `HOME`, `USER`, `LOGNAME`, `TMPDIR` y `PATH` solo con entradas absolutas. En Windows conserva las variables de sistema de `invoke.rs` y `ProgramFiles`.
  - La resolución de Git y los hijos `git` solo ven esa captura. Una prueba estática impide leer el entorno en el módulo `daemon` fuera de `env.rs`.
  - `XDG_DATA_HOME`, `XDG_CONFIG_HOME` y `XDG_STATE_HOME` se ignoran en `ProfileDirs::resolve`, la única función de resolución del perfil para el daemon, la CLI y el dispatcher. Es la enmienda del 2026-10-04 a ADR-GRP-006 § 1.
  - Las variables del cargador (`LD_PRELOAD`, `DYLD_*`) solo afectan al arranque del propio proceso, que queda fuera de alcance según la TS. Una librería inexistente hace que dyld aborte antes de `main`. No llegan a los hijos.
- **SEC-13**: la marca `running` y la clasificación de § 4. La autorización de `stop` como comando reservado es de TS-GRP-004.

## 6. Plan de pruebas (criterio → test)

Todas usan perfiles temporales (`ProfileDirs::under_root`, o `GITRAPTOR_PROFILE_DIR` en el binario debug) y repos temporales. Nunca este repo ni el perfil real (NFR-01). Las pruebas de proceso lanzan el binario real `raptor`.

| Criterio de la TS | Test |
|---|---|
| Instancia única | `apps/cli/tests/daemon_process.rs::single_instance_two_simultaneous_daemons`, `crates/core/tests/daemon_lifecycle.rs::second_daemon_exits_without_touching_the_store` (el almacén y el log no cambian), `daemon::lock::tests::*` (incluida `children_do_not_inherit_the_lock`) |
| Esqueleto de estados | `daemon_lifecycle::reaches_observing_with_a_repo_and_valid_git`, `daemon_lifecycle::declared_states_without_repos_or_without_git`, `daemon::state::tests::*` (solo las 6 transiciones de BR-WF-002) |
| Parada ordenada | `daemon_process::daemon_stop_is_orderly`, `daemon_process::termination_signal_is_orderly`, `daemon_lifecycle::orderly_stop_persists_observed_until_and_releases_the_lock`, `daemon_lifecycle::heartbeat_persists_observed_until_while_observing` |
| Caída (SEC-13) | `daemon_process::kill_9_during_active_session_is_marked_and_lock_recovers`, `daemon_lifecycle::crash_during_active_session_is_marked_on_next_start`, `crash_without_sessions_is_a_plain_daemon_down_gap`, `authorized_stop_command_is_an_attributed_stop`, `daemon::tests::*` |
| Entorno mínimo | `daemon_process::starts_with_an_empty_path` |
| Logs (SEC-05) | `daemon_process::logs_never_contain_content_secrets_or_paths` (contenido marcado, secretos en el entorno y en el nombre del repo; log, stdout y stderr), `crates/core/tests/daemon_panic_redaction.rs`, `daemon::log::tests::*` (rotación, 0600, campos cerrados) |
| Entorno hostil (SEC-10) | `daemon_process::hostile_environment_does_not_change_behavior` (`GIT_EXEC_PATH`, `GIT_DIR`, `GIT_CONFIG_PARAMETERS`, `LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`, `PATH=.:bin:…` con un `git` falso, `XDG_CONFIG_HOME`), `daemon::env::tests::*` (incluida la prueba estática) |
| Repo intacto (suite "Proceso") | `daemon_process::repo_and_outside_stay_intact`: huella del repo idéntica, nada fuera del perfil, lock y log 0600 en `state/` |
| Permisos (SEC-06) | `daemon_lifecycle::insecure_state_folder_stops_the_start`, `lock::tests::insecure_lock_file_is_refused_not_fixed`, `symlinked_lock_file_is_refused` |

## 7. Fuera de alcance y pendientes

- **TS-GRP-004**:
  - El canal y el handshake.
  - El arranque bajo demanda con entorno limpio.
  - La autorización de `daemon stop` como comando reservado, con su prueba "agente → rechazado".
  - La causa `stop-command` con el cliente, que conecta con `ShutdownHandle::request(StopCause::StopCommand { .. })`.
  - En Windows, la parada ordenada y `daemon stop`.
- **US-GRP-004**:
  - `raptor daemon enable`/`disable` y SEC-14. **No se registra ningún autoarranque** (PQ-1).
  - El artefacto del gestor de servicios debe excluir el código 3 del relanzamiento (`SuccessfulExit`/`RestartPreventExitStatus=3`).
- **US-GRP-005**:
  - Registrar el hueco de `StartupReport::repos` antes del primer latido.
  - La reconciliación.
- **US-GRP-014 y US-GRP-015**: entrar y salir de "Esperando Git" y de "Sin repos" y exponerlos. **US-GRP-002**: el observador.
- **INF-GRP-001**: conectar la suite "Proceso" al núcleo del arnés cuando esté en `main`. Hoy la huella es propia de `apps/cli/tests`. También queda allí la auditoría dinámica de `exec`.
- **Riesgo residual (`HOME`)**: `directories` lee `HOME` para resolver el perfil, así que un `HOME` hostil lo redirige. Sacarlo de la base de usuarios (`getpwuid_r`) exige FFI o un crate nuevo. Se propone para TS-GRP-004, donde el arranque bajo demanda ya fija el entorno.
- **Windows**: la notificación de cierre de sesión llega como mensaje de ventana, así que hasta entonces un logoff cuenta como caída. Faltan además las ACL del lock y de los logs. Nada de esto se verificó desde macOS.
- **Linux**: no se verificó en esta rama (solo macOS).
- `cargo-deny` no está configurado en el repo. `signal-hook` es MIT o Apache-2.0 y no se pasó por `cargo-deny`.
