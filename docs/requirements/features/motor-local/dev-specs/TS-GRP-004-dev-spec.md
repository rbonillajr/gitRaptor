---
id: DS-TS-GRP-004
title: "Dev Spec — Canal local de clientes y contrato de mensajes"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-04
updated: 2026-10-05
related:
  stories: [TS-GRP-004]
  adrs: [ADR-GRP-005, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013]
  nfrs: [NFR-02, NFR-03, SEC-01, SEC-02, SEC-03, SEC-08, SEC-10, SEC-12, SEC-13, SEC-14]
  deps: [DEP-CKP-6, DEP-MCP-3]
tags: [motor-local, ipc, json-rpc, socket, contrato, comandos-reservados, auditoria, arranque-bajo-demanda, seguridad]
---

# Dev Spec — TS-GRP-004: Canal local de clientes y contrato de mensajes

Plano de ejecución compacto (AADD ligero) de [TS-GRP-004](../technical-stories/TS-GRP-004-canal-clientes.md). La forma del canal la fija [ADR-GRP-005](../../../../architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 3, § 5 y § 6, con su [Enmienda TS-GRP-004](../../../../architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md#enmienda-2026-10-04-ts-grp-004). El contrato se describe en [api-contract-ipc.md](../../../../architecture/design/api-contract-ipc.md). Se apoya en el daemon de TS-GRP-003 (`crates/core::daemon`) y en el almacén del perfil de TS-GRP-001.

## 1. Decisiones

Todas son **Decisión del orquestador (2026-10-04), validada por Arquitecto** (técnica) **y PO** (alcance). La columna de la derecha recoge los ajustes que pidieron y que ya están incorporados.

| # | Decisión | Ajuste incorporado |
|---|---|---|
| D1 | Socket Unix con `std`, un hilo por conexión. JSON-RPC 2.0 con un mensaje por línea, 1 MiB máximo, profundidad 32 (contada antes de serde), batches rechazados, `deny_unknown_fields` en todos los tipos, `hello` obligatorio con un timeout de 2 s | El handshake presenta el id de instancia del perfil. El cliente comprueba con `LOCAL_PEERCRED` que el servidor es su mismo uid. Tope de 32 conexiones más 2 plazas solo para clientes con terminal de control, y 8 por proceso cliente `(pid, inicio)` |
| D2 | Identidad no reutilizable del llamante en macOS: `(pid, hora de inicio en µs)`, la misma de ADR-GRP-012. Cuadra con el audit token (pid y euid). Se comprueba que el inicio del cliente es anterior al `accept`, que cada padre es anterior a su hijo y que el cliente sigue siendo el mismo al terminar el recorrido. **Se desvía del audit token** de ADR-GRP-005 § 6.1 (Enmienda) | Evaluación en cada petición, no en el `accept`. Riesgo residual registrado: reutilización del PID entre `connect` y `accept` |
| D3 | Crates `nix` 0.31.3 (opciones de socket y passwd) y `libproc` 0.14.11 (macOS: `proc_pidinfo`, `proc_pidpath`), ambos fijados con `=`. Se descarta `darwin-libproc` (fija `memchr ~2.3`, incompatible con gix) y `sysinfo` (no da terminal de control) | `libproc` genera sus bindings con bindgen y necesita el SDK de Command Line Tools para compilar en macOS. `cargo-deny` sigue sin configurar en el repo |
| D4 | Clasificador único de ejecutables: es Claude Code si se llama `claude`, si vive en `…/claude/versions/<versión>` (instalador nativo) o si su ruta pasa por `@anthropic-ai/claude-code`. Un intérprete (`node`, `bun`, `deno`) se trata como agente (**fail-closed**) hasta que SPIKE-GRP-001 resuelva la lectura de argv[1]. El recorrido solo para en el pid 1 o en un proceso de **otro euid**. Un proceso del mismo uid que no se puede leer rechaza el comando. Override solo en debug: `GITRAPTOR_AGENT_EXECUTABLES`, que deja aviso en el log | En Terminal.app, `login` es setuid root (euid 0 con el ruid del usuario), así que se clasifica por **euid**. Medido en esta máquina |
| D5 | Controles del daemon, todos fail-closed. Identidad estable. Ni el cliente ni sus antecesores son agentes. Tampoco el líder de su sesión ni los antecesores de este. El cliente tiene terminal de control | El líder de sesión de otro euid (`login`) cuenta como "no agente". **No** se exige que el líder sea del mismo uid, como propuso el Arquitecto: en Terminal.app el líder es `login` (root) y la regla dejaría fuera a todos los desarrolladores. Medido |
| D6 | Comandos reservados: `daemon.stop`, implementado. `repo.add` (US-GRP-001), `repo.retire` (US-GRP-006), `attribution.correct` y `attribution.withdraw-correction` (US-GRP-010) y `registration.withdraw` (US-GRP-009) se declaran en el registro de métodos del contrato. El daemon los valida, los autoriza, los audita y responde `-32004` con la historia que los implementa | PO: el registro de agente (worktree = cwd del llamante) y "retirar el propio registro" pasan a US-GRP-009, con enmienda de la TS. El validador de nombres declarados ya está aquí (`channel::validate::declared_agent_name`) |
| D7 | Auditoría: migración 2 del índice global, tabla `reserved_audit` append-only (triggers que abortan `UPDATE` y `DELETE`). Guarda la operación, el repo, el resultado, el motivo, el cliente y la cadena de ascendencia. Consulta `audit.list` (fuera del MCP) y evento `reserved.audit` | Si la auditoría no se puede escribir, el comando no se ejecuta |
| D8 | `PROTOCOL_VERSION = 1`, con compatibilidad por igualdad. Un cliente más nuevo envía `daemon.replace`. El daemon lo acepta sin los controles de reservado solo si **el archivo que hay ahora en su ruta de lanzamiento cambió desde que arrancó** (`dev`, `inode`) **y el ejecutable del llamante es ese archivo nuevo**. Si no, lo trata como `daemon.stop` reservado. `StopCause::Replace` se clasifica como `daemon-stopped` con el cliente | Cubre la carrera con `exec` y el upgrade de brew (cada versión en su carpeta del Cellar) |
| D9 | Arranque bajo demanda: `<carpeta del ejecutable>/raptor daemon` con `env_clear`. `HOME`, `USER` y `LOGNAME` salen de la base de usuarios (`getpwuid`) y `PATH` es fijo (`/usr/bin:/bin:/usr/sbin:/sbin`). En debug pasan además los overrides de perfil y de agente. cwd en `state/`, stdio a null; el cliente recoge al hijo. El daemon hace `setsid()` al arrancar | Sesión y grupo propios: el MCP bajo Claude Code no lo arrastra al salir. `TMPDIR` no se pasa (el daemon no lo usa) |
| D10 | SEC-08: cola de salida de 1024 mensajes por conexión. Si se desborda, se vacía, se encola `events.resync` y se desconecta, sin bloquear nunca al productor. Escritura con timeout de 2 s, 4 suscripciones por conexión y token bucket de 100/s con ráfaga de 200 (`-32005`, sin desconectar). Consultas desde memoria | — |
| D11 | Ruta larga del socket (más de 100 bytes): `bind` y `connect` relativos a `run/`, cambiando el cwd bajo un mutex de proceso. Tras el `chdir` se verifica `.` (propietario y modo 0700). El `bind` ocurre en `Daemon::start`, antes de que el daemon cree hilos | — |
| D12 | `gitraptor_api::clock::monotonic_ns()` con `CLOCK_MONOTONIC` (en macOS, sobre `mach_continuous_time`). El sobre del evento lleva `seq`, `kind`, `version`, `wall_ms`, `timings` y `data`. Los eventos de cambio llevan `timings` siempre, y `t_published` lo pone el bus. Eventos propios: `engine.state`, `daemon.stopping`, `reserved.audit`. Los de las historias se declaran versionados, sin `data` | Windows (QPC): pendiente |
| D13 | `Untrusted` viaja como `{"untrusted": "…"}` con las marcas `truncated` y `lossy` (texto no UTF-8). `sanitized()` quita CSI, OSC, DCS, C0, C1, bidi y caracteres de ancho cero. El daemon no sanea; limpian los clientes | — |
| D14 | `Actor::Agent { kind, name, origin }` o `Actor::Unattributed`. Una prueba sobre el JSON Schema (schemars) comprueba que no aparece "human" | — |
| D15 | Perfil MCP fijado por el ejecutable del par (`raptor-mcp`) o por lo declarado en `hello`. En perfil MCP, los reservados se rechazan (`not-available-to-mcp`, auditado) y `audit.list` no existe. La instantánea es `McpSnapshot` (el tipo es la allowlist): `run_id`, `seq`, `engine_state` y `caller_repo` (el repo observado que contiene el cwd del llamante) | En macOS `caller_repo` es siempre `null`: `libproc` no da el cwd de otro proceso sin `unsafe`. Pendiente con F-001-05 y US-GRP-009 |
| D16 | CLI: `raptor daemon stop` pasa por el canal con confirmación interactiva (`--yes` la salta; sin TTY y sin `--yes`, se niega). Nuevo `raptor daemon status`, de diagnóstico y solo lectura, que arranca el daemon bajo demanda y muestra la instantánea saneada. `raptor-mcp` arranca el daemon, hace el handshake y termina. Se elimina la parada provisional por SIGTERM (`signal_running_daemon`) | PO: `status` sin compromiso de presentación (F-001-02), con mensajes en/es, y anotado en la TS como añadido |
| D17 | DEP-CKP-6: `engine.snapshot` (con `run_id` y `seq` N leídos bajo el mismo lock con el que se publica) y `events.subscribe { from_seq: N+1, run_id }` con un buffer de reproducción de 1024 eventos. Si el `run_id` es otro o la secuencia ya salió del buffer: `events.resync` | DEP-CKP-2 y DEP-CKP-3 (consultas bajo demanda) esperan la enmienda de ADR-GRP-005 § 5, que debe conservar caché y rate limit. DEP-CKP-4, DEP-CKP-5 y DEP-CKP-11, pendientes de sus historias |
| D18 | SEC-02: validadores de ruta (léxicos antes de tocar el FS; luego canonicalizar y comprobar contra los worktrees observados) y de ref (`crates/git::refname`), probados en unidad. Por el canal se prueban con `repo.add`. Fuzz: prueba de propiedades del decodificador, 20 000 entradas mutadas | Sin `cargo-fuzz`, que necesita nightly. Se dice en el PR |
| D19 | ADR-GRP-005: sección propia "Enmienda (2026-10-04, TS-GRP-004)" con D2, D4, D5, D8, D9, D11, DEP-MCP-3 y el pendiente de Windows. No toca el resto del ADR | El Arquitecto recomienda que el security-expert revise D2 y D8 |
| D20 | Gate de seguridad de `non-functional.md`: mismo criterio que TS-GRP-003 D9 | — |
| D21 | **DEP-MCP-3 (confused deputy)**: el daemon conoce su propia identidad `(pid, inicio)`. Un llamante que la tiene en su ascendencia o en la de su líder de sesión se rechaza con `daemon-descendant` y queda auditado con `daemon_descendant = true` | Basta la ascendencia: registrar el pgid o los PIDs de los hijos no resiste `setsid` ni el doble fork, que son el riesgo residual de ADR-GRP-005 § 6, ahora extendido a los descendientes del daemon. Requisitos para el futuro ejecutor (DEP-CKP-7, ADR-TMC-002 § 5): (1) un registro de operaciones en curso con el solicitante y la identidad del hijo, para atribuir el intento; (2) contención donde el SO la ofrezca (`PR_SET_CHILD_SUBREAPER` en Linux, Job Object sin breakaway en Windows; macOS sin equivalente); (3) nada heredable: ni terminal de control ni descriptores del canal (CLOEXEC); (4) una prueba de un hook con doble fork, que en Linux se rechaza |

### Revisión de seguridad (security-expert, 2026-10-04)

La revisión fue estática y no encontró nada crítico. Los cambios que generó ya están en el código y en esta spec.

| Hallazgo | Tratamiento |
|---|---|
| A-1 · Un agente llena las conexiones (pty para las plazas extra, varios procesos, saludo lento) | **Corregido**. Las 2 plazas extra son para quien pasa los controles de reservado, no para quien tiene una pty. El handshake tiene un plazo total de 2 s. Una conexión sin suscripciones se cierra tras 60 s de inactividad |
| A-2 · Lanzar el comando a través de otra aplicación (`open x.command`, `tmux new-window`, la terminal de Orca) da una ascendencia limpia | **Riesgo del modelo, no del código**: se registra en la Enmienda de ADR-GRP-005 (punto 9). Control compensatorio previsto: cada reservado aceptado se publica en `reserved.audit` para que el Cockpit lo muestre. **Bloquea el release** de `repo.retire` y `attribution.correct` hasta que haya decisión |
| M-1 · Un `daemon.replace` falso se registraba como parada atribuida | **Corregido**. La marca guarda `replace:<protocolo>` y el siguiente arranque solo la cuenta como `daemon-stopped` si su protocolo es mayor. Si no, es una caída |
| M-2 · Otro proceso cambia el socket y suplanta al daemon | **Corregido**. En cada latido el daemon compara `(dev, inode)` del socket y, si cambió, lo registra (`channel_socket_replaced`), cierra las conexiones y vuelve a enlazar. El bucle de `accept` se despierta con una tubería, no conectando al socket. La CLI sanea todo lo que imprime del daemon |
| M-3 · Inundación de la auditoría | **Corregido en parte**: como máximo 1 reservado por segundo por conexión (ráfaga de 5), y el resto se rechaza con `-32005` y deja aviso en el log. La retención de la auditoría queda pendiente (sin caducidad por ADR-GRP-013) |
| B-1 · Un suscriptor MCP recibía `reserved.audit` | **Corregido**. Los suscriptores MCP no reciben ese tipo de evento. Declarar `client: cli` sigue dando el perfil completo: el perfil no es una frontera de seguridad, y los reservados se autorizan aparte |
| B-2 · Los overrides de debug (`GITRAPTOR_AGENT_EXECUTABLES`, `GITRAPTOR_PROFILE_DIR`) existen en cualquier build debug | **Pendiente**: pasarlos a una feature `test-hooks`, para los dos a la vez (el de perfil viene de TS-GRP-001). Un release no los lee |
| B-3 · Copia de `params` en el decodificador | **Corregido**: se deserializa desde una referencia |

## 2. Dependencias

| Crate | Versión | Dónde | Por qué |
|---|---|---|---|
| `nix` | `=0.31.3` (`socket`, `user`) | `crates/core`, `cfg(unix)` | `LOCAL_PEERCRED`, `LOCAL_PEERPID` y `LOCAL_PEERTOKEN` sin `unsafe` propio, y `getpwuid` para el entorno limpio. MIT |
| `libproc` | `=0.14.11` | `crates/core`, solo macOS | `proc_pidinfo(PROC_PIDTASKALLINFO)` (ppid, euid, flags, inicio) y `proc_pidpath`. MIT. Usa bindgen al compilar |
| `serde`, `serde_json`, `schemars` | ya en el árbol | `crates/api`, `crates/core` | Contrato y JSON Schema |
| `rustix` (`time`, `net`) | ya en el árbol | `crates/api`, `crates/core` | Reloj monótono, `getsid`, `setsid`, `SO_PEERCRED` (Linux) |

## 3. Estructura

| Módulo | Responsabilidad |
|---|---|
| `crates/api::{rpc, framing, methods, messages, event, untrusted, actor, clock}` | El contrato, sin E/S: sobres JSON-RPC, lectura y decodificación acotadas, registro de métodos, mensajes, eventos, texto no confiable, actor y reloj |
| `crates/core::channel::transport` | `bind` y `connect` del socket (carpeta privada, socket 0600, uid del servidor, ruta larga) |
| `crates/core::channel::peer` | Credenciales del par y lectura de procesos (`ProcSource`, implementado para macOS y Linux) |
| `crates/core::channel::authz` | Clasificador de agentes y `check_reserved`, que devuelve el veredicto, la identidad y la cadena |
| `crates/core::channel::bus` | `EventBus` (secuencia, reproducción, vista del motor bajo un lock) y `Outbox` acotada |
| `crates/core::channel::conn` | Aceptación (uid, límites), handshake, despacho, comandos reservados y auditoría |
| `crates/core::channel::validate` | Rutas, refs y nombres declarados (SEC-02, M7) |
| `crates/core::client` | `Client`, `ensure_daemon` (arranque bajo demanda y reemplazo) y `clean_env` |
| `crates/core::daemon` | `ChannelConfig` en `DaemonConfig`; `bind` en `start`; servicio, auditoría y parada en `run`/`stop`; `Control` sustituye a `StopCause` en el canal interno; `setsid` en `run_process` |
| `apps/cli`, `apps/mcp` | `raptor daemon stop` por el canal con confirmación, `raptor daemon status` y conexión de `raptor-mcp` |

## 4. Plan de pruebas (criterio → test)

Todas usan perfiles y repos temporales, nunca este repo ni el perfil real (NFR-01). El agente simulado es una copia del binario de pruebas llamada `raptor-fake-agent`, declarada como agente con el override de debug. Así, la sesión real de Claude Code que ejecuta las pruebas no se confunde con el agente.

| Criterio de la TS | Test |
|---|---|
| Bajo demanda (dos clientes, un daemon) | `apps/cli/tests/channel_process.rs::two_clients_at_once_start_a_single_daemon_on_demand`, `apps/mcp/tests/on_demand.rs::raptor_mcp_starts_the_engine_on_demand` (sustituida por US-MCP-001: según ADR-MCP-001 § 1, `raptor-mcp` ya no arranca el motor al iniciar; vuelve con la primera herramienta, US-MCP-003) |
| Versión (SEC-13) | `crates/core/tests/channel.rs::a_newer_installed_binary_replaces_an_older_daemon`, `replace_from_another_executable_is_a_reserved_stop`, `a_newer_daemon_tells_an_old_client_to_update` |
| Acceso (SEC-01) | `channel_process::the_channel_is_private_and_never_a_network_port`, `a_precreated_open_socket_folder_stops_the_daemon`, `channel::socket_is_private_and_handshake_presents_the_instance`, `a_client_of_another_user_is_rejected` (con `expected_uid`: sin root no se puede cambiar de usuario), `a_precreated_open_runtime_folder_stops_the_start` |
| Red (NFR-03) | `channel_process::the_channel_is_private_and_never_a_network_port` (`lsof -i` vacío y `lsof -U` con el socket) |
| Entradas (SEC-02) | `channel::malformed_input_is_refused_without_stopping_the_daemon`, `framing::tests::decoder_never_panics_on_random_input`, `validate::tests::*` (UNC, dispositivos, ADS, traversal, symlink, `--upload-pack=x`) |
| Comandos reservados (SEC-03) | `channel_process::reserved_commands_from_an_agent_are_refused_and_audited` (CLI, pty y JSON-RPC directo con `nc -U` bajo el agente; luego el desarrollador para y queda `accepted`), `channel::reserved_commands_from_an_agent_descendant_are_refused_and_audited` (los cinco reservados), `authz::tests::*` |
| Confused deputy (DEP-MCP-3) | `channel::a_child_of_the_daemon_cannot_use_reserved_commands`, `authz::tests::a_descendant_of_the_daemon_is_refused` |
| Auditoría | `channel::the_audit_is_append_only`, más las dos pruebas de reservados |
| Robustez (SEC-08) | `channel::slow_client_and_connection_flood_do_not_starve_the_others` (cliente que no lee y 100 conexiones; p95 de `t_published → t_client_recv` < 25 ms), `requests_beyond_the_rate_limit_are_refused_not_disconnected`, `bus::tests::*` |
| Arranque limpio (SEC-10) | `channel_process::an_on_demand_daemon_does_not_inherit_the_client_environment` (entorno del daemon leído con `ps eww`, `HOME` hostil, `PATH` relativo, grupo propio) |
| Salida (SEC-12) | `channel_process::untrusted_repo_text_is_printed_sanitized`, `channel::repo_text_is_marked_untrusted_in_the_contract`, `channel::mcp_connections_get_the_allowlist_only`, `messages::tests::mcp_snapshot_is_the_field_allowlist`, `untrusted::tests::*` |
| Stream | `channel::stream_is_ordered_gapless_and_timed`, `engine_state_is_the_first_event_of_a_run`, `a_restarted_daemon_asks_for_a_resync`, `event::tests::change_events_must_carry_timings` |
| Actor | `actor::tests::schema_has_no_human_variant` |
| Ruta larga en macOS | `channel::a_profile_path_beyond_the_socket_limit_still_connects` |
| Parada por el canal (TS-GRP-003) | `daemon_process::daemon_stop_is_orderly` (ahora `stop-command` con el cliente), `channel_process::a_stop_without_a_controlling_terminal_is_refused`, `stop_confirmation_needs_a_terminal` |

**Verificación manual** (2026-10-04, macOS, desde esta sesión real de Claude Code, con el clasificador real): `raptor daemon stop --yes` directo y bajo `script` (pty) contra un daemon de perfil temporal se rechazan con `agent-ancestry`. Desde la terminal del desarrollador (pty, sin agente) la parada se acepta. No se probó con un `repo add` de CLI, porque no existe (D6).

## 5. Pendientes

- **Windows** (Pendiente: etapa de validación multiplataforma): named pipe con DACL del SID, primera instancia, rechazo de clientes remotos, comprobación del SID del servidor y SQOS de identificación; identificador por handle; reloj por QPC. Hoy el crate compila en Windows (`clippy -D warnings` con `--target x86_64-pc-windows-msvc`), pero allí no hay canal: el daemon arranca sin él y deja `channel_unsupported` en el log, y el cliente falla con el error tipado `ClientError::TransportUnsupported` (`channel::TRANSPORT_UNSUPPORTED`). Es fail-closed: nunca hay un canal sin control de acceso. **Decisión del orquestador (2026-10-04)**. Requiere Win32 sin `unsafe` propio (crate a evaluar).
- **Linux** (Pendiente: etapa de validación multiplataforma): `SO_PEERCRED` y `/proc` están escritos pero no se compilaron ni se probaron. El inicio se calcula con 100 Hz supuestos y falta pidfd (`SO_PEERPIDFD`). Las pruebas de proceso son solo de macOS (`script`, `nc -U`, `lsof`).
- **SPIKE-GRP-001**: leer argv[1] de un intérprete. Hasta entonces, un antecesor `node`, `bun` o `deno` bloquea los comandos reservados (fail-closed).
- **Riesgo para que Rene lo acepte**: si Claude Code corre sobre `node`, solo se detecta gracias al fail-closed de los intérpretes. Un agente con otro intérprete o desacoplado (doble fork, `setsid`) evade la ascendencia (riesgo residual de ADR-GRP-005 § 6).
- **US-GRP-009**: tomar el worktree del cwd del llamante (en macOS falta un wrapper seguro de `PROC_PIDVNODEPATHINFO`), registrar con nombre validado y las pruebas de worktree ajeno y de retiro propio o ajeno.
- **US-GRP-004**: arranque vía `launchctl kickstart` o `systemctl --user start` cuando el autoarranque esté registrado.
- **F-001-05**: allowlist de herramientas MCP y `caller_repo` real.
- **Ejecutor de operaciones (DEP-CKP-7)**: los cuatro requisitos de D21 (DEP-MCP-3).
- **Cockpit**: DEP-CKP-2 y DEP-CKP-3 (enmienda de ADR-GRP-005 § 5), DEP-CKP-4, DEP-CKP-5 y DEP-CKP-11.
- **INF-GRP-001**: conectar estas pruebas de proceso a la suite "Proceso" del arnés.
- `cargo-deny` y `cargo-fuzz` no están configurados en el repo.
- Seguridad: A-2 (decisión de Rene), B-2 (feature `test-hooks`) y la retención de la auditoría.

## 6. Enmienda (2026-10-05): N1 a N7 de ADR-CKP-003 § 4 (protocolo 5)

Aplica la enmienda **E6** de [ADR-CKP-003](../../../../architecture/decisions/ADR-CKP-003-arquitectura-tui.md): lo que el esqueleto de la TUI (INF-CKP-001, hito M1) necesita del canal. No cambia las decisiones D1 a D21. Todas son **Decisión del orquestador (2026-10-05), validada por el Arquitecto**.

### 6.1 Decisiones

| # | Decisión | Nota |
|---|---|---|
| E-D1 | **Protocolo 5** (`API_VERSION` 5.0.0) con **ventana de compatibilidad**: el daemon acepta clientes de `MIN_COMPATIBLE_PROTOCOL = 4` a `5` y la conexión recuerda el protocolo negociado. Una conexión 4 no ve lo nuevo: `HelloResult` sin `requester`, `methods` sin los métodos de la 5 (le responden `-32601`) y el resto del cable sin cambios. Un cliente 5 ante un daemon 4 recibe `-32002` y lo reemplaza, como hasta ahora (D8, SEC-13). Motivo: un `raptor-mcp` de larga vida bajo Claude Code sobrevive a una actualización | `MethodSpec.since`; `ChannelConfig.min_protocol` configurable para las pruebas |
| E-D2 | **Ámbitos (N1, N2)**: `Scope` = `{"scope":"global"}` o `{"scope":"repo","repo_id":…}`. Cada tipo de evento declara su ámbito en el registro (`EventScope`). El bus asigna, bajo el mismo lock que la secuencia global, una **secuencia contigua por ámbito** (`scope_seq`). Globales: `engine.state`, `daemon.stopping`, `reserved.audit`, `repo.observation`, `repo.attention`. Del repo: `worktree.state`, `git.event`, `operation.*` y los declarados por historias (su `data` lleva `repo_id`) | `engine.snapshot` y `events.subscribe` siguen igual |
| E-D3 | `scope.snapshot { scope }` → global `{run_id, scope_seq, engine, daemon, autostart, repos: [RepoSummaryView]}` o repo `{run_id, scope_seq, repo: RepoView}`, leídos bajo el lock con el que se publica. La de repo recuenta el ahead/behind y respeta el presupuesto de mensaje, como `engine.snapshot`. Repo desconocido: `-32009` | — |
| E-D4 | `scope.subscribe { scope, from_seq?, run_id? }` → `{subscription, scope, from_seq}`; notificaciones `scope.event { subscription, scope, scope_seq, event }`. Mismo buffer de reproducción (1024); el bus guarda por ámbito la última `scope_seq` expulsada. Si `from_seq` ya salió o el `run_id` es otro: **`scope.resync { scope, reason }`** y `-32007`. `events.unsubscribe` vale para las dos clases | El cliente lento sigue siendo de conexión (`events.resync`, `slow-consumer`) y vale para todos sus ámbitos |
| E-D5 | **N3**: `RepoSummaryView { repo_id, state, path, attention }`, `attention = { conflicts, denials, gaps }`, cada uno `counted {count}` o `unavailable {reason: not-published}`. Hoy los tres son `unavailable` (predictor TS-CKP-001, Guardrails, US-GRP-005); el evento global `repo.attention` queda declarado con su tipo, sin emisor. `autostart`: `registered`, `not-registered` o `unknown`; hoy `unknown` (registro de US-GRP-004) | BR-CKP-CALC-001: lo no publicado es "no disponible", nunca cero |
| E-D6 | **N4**: `repo.locate { path }` → `{ repo_id, worktree }`. Validación léxica antes de tocar el FS (SEC-02), luego canonicalizar y elegir el worktree observado que la contiene (la raíz más larga). Fuera o inexistente: `-32009`. No reservado; fuera del MCP (devuelve rutas) | — |
| E-D7 | **N5**: `HelloResult.requester` (protocolo 5, perfil completo, cliente `cli`) = `resolved {actor, layer, confirmable}` o `unverified`. Misma resolución que `requester.resolve` y misma capa del ejecutor (ADR-CKP-002 § 4). Solo UX: el daemon re-resuelve en cada petición | No se resuelve para `other` ni `mcp` (el cliente de hooks no paga el recorrido) |
| E-D8 | **N6**: `Untrusted<const MAX>` con dos clases: `Untrusted` (rutas, 4096 bytes) y `UntrustedName` (ramas, nombres de worktree y de agente, 1024 bytes). Mismo cable; el JSON Schema lleva `maxLength` y **el decodificador impone el tope** (recorta y marca `truncated`), también en el cliente | Compatible con la 4 |
| E-D9 | **N7**: `StoppingData.cause` pasa a enum (mismas cadenas); `rpc::ErrorCode` enumera todos los códigos (`ALL`, `from_code`); los errores que solo traían texto llevan `data.reason` tipado (`-32602` por ruta: `InvalidReason`; `-32010`: `ScopeRefusal`); lo nuevo de N1 a N5 es solo enums. Un cliente presenta `code` y `data`, nunca `message` | El catálogo en/es de la CLI tiene una clave por variante nueva, con prueba |
| E-D10 | **L-06**: la comprobación del par vive en la biblioteca cliente (`channel::transport::connect`): además del uid del servidor, la carpeta del socket debe ser una carpeta real (no un enlace simbólico) del usuario y en 0700, comprobada con `fsperm::verify_private_dir` antes del `connect`, con ruta corta o larga. Si no, `ClientError::ChannelRejected` sin enviar nada | Arquitecto (A1): la biblioteca cliente vive hoy en `crates/core` (`client.rs`, `channel::transport`, `channel::peer`), y ADR-CKP-003 § 5 y V5 prohíben que la TUI importe `gitraptor_core`. **Pendiente, dueño: INF-CKP-001**: sacar el cliente de `crates/core` (a `crates/api` o detrás de una fachada) antes de que la TUI lo use |
| E-D11 | **CLI**: `raptor daemon status` muestra "actúas como" con la capa y el autoarranque, en en/es | El resto de presentación es de INF-CKP-001 |
| E-D12 | **N8 a N11 (Should)**: N11 ya lo cubre TS-CKP-002 (huella del plan y revalidación). N8, N9 y N10 quedan pendientes para la historia que los use | — |

**Ajustes del Arquitecto incorporados** (revisión del 2026-10-05):

- **A1** (frontera V5): ver E-D10.
- **A2** (repo que deja de observarse): con `repo.observation { observed: false }`, las suscripciones de ese repo reciben `scope.resync { reason: scope-closed }` y terminan. Su `scope_seq` no se reinicia dentro del mismo `run_id`, así que un `from_seq` viejo nunca reproduce eventos que no son. **Desvío razonado**: el suelo del ámbito se conserva en lugar de borrarse. Sin el suelo, un `from_seq` antiguo de un repo vuelto a añadir podría pedir eventos ya expulsados del buffer y no recibir el `resync`. El coste es una entrada por repo observado en la ejecución.
- **A3**: `scope.snapshot`, `scope.subscribe` y `repo.locate` no se ofrecen al perfil `mcp` (llevan rutas, SEC-12): no aparecen en `hello.methods` y responden `-32601`. Las suscripciones por ámbito cuentan dentro del límite de 4 por conexión y comparten espacio de ids con `events.subscribe`.
- **A4**: prueba de un cliente 4, ya en `Ready`, que intenta `daemon.replace`. Se rechaza con `-32602` y el daemon sigue en marcha.
- **Opcionales adoptados**: `HelloResult.protocol` lleva el protocolo **negociado**. El `data.reason` de `-32602` y `-32010` es aditivo: un cliente 4 lo tolera porque `ErrorObject.data` es un `Value` genérico. La TUI no avisa de "sin autoarranque" cuando el valor es `unknown` (nota para INF-CKP-001). Si llega `-32012`, el cliente vuelve a llamar a `requester.resolve`.
- **Opcionales no adoptados, anotados**:
  - `since` en `EventKind`: hoy ninguna conexión 4 recibe un tipo nuevo, porque `repo.attention` no tiene emisor. Lo añade quien emita el primer tipo nuevo.
  - Plazo o filtro previo para `canonicalize` en volúmenes de red: riesgo anotado.
  - `ErrorCode::Other`: un código desconocido devuelve `None` y la CLI muestra el `message` saneado.
  - `ScopeRefusal` tipado: no añade información. Antes ya viajaba el mismo motivo como texto en `message`, y no distingue "observado pero fuera de la allowlist" de algo que el agente no supiera ya (ADR-MCP-001 § 5).

### 6.2 Plan de pruebas (criterio → test)

Daemon y cliente reales sobre perfiles y repos temporales del testkit (NFR-01). Sin esperas fijas: cada prueba espera la notificación o respuesta que demuestra el hecho, con un plazo solo como techo.

| Criterio | Test (`crates/core/tests/channel_scopes.rs` salvo indicación) |
|---|---|
| N1 global | `global_snapshot_then_subscribe_is_gapless` |
| N1 repo, con un cambio real | `repo_snapshot_then_subscribe_sees_a_real_change_without_gaps` |
| N2 contigua por ámbito | `each_scope_has_its_own_contiguous_sequence` |
| N2 `scope.resync` con causa | `a_lost_replay_or_another_run_gets_a_scoped_resync` |
| N2 cliente lento | `bus::tests::slow_subscriber_is_dropped_without_blocking`, `bus::tests::scoped_subscriber_that_does_not_read_gets_a_connection_resync` |
| N2 suelo por ámbito | `bus::tests::scope_sequences_are_contiguous_and_replay_has_a_floor` |
| N2 repo retirado (A2) | `a_retired_repo_closes_its_scope` |
| N3 | `global_snapshot_has_attention_and_autostart` |
| N4 | `repo_locate_finds_the_observed_worktree`, `repo_locate_refuses_what_is_not_observed` |
| N5 | `hello_says_who_the_caller_is` |
| N6 | `untrusted::tests::decoder_enforces_the_field_bound`, `schema_declares_each_bound` |
| N7 | `rpc::tests::every_code_is_enumerated`, `rpc::tests::typed_reasons_round_trip`, `messages::tests::stop_cause_is_a_code`, `conn::tests::invalid_path_and_scope_have_typed_reasons`, `event::tests::every_kind_has_a_scope` |
| N7 i18n | `apps/cli` `codes::tests::every_contract_code_has_both_messages`, `codes::tests::requester_and_autostart_are_presented_from_codes` |
| Compatibilidad 4 ↔ 5 | `a_protocol_4_client_still_works_with_a_protocol_5_daemon`, `an_older_client_cannot_replace_a_newer_daemon` (A4), `channel::a_newer_installed_binary_replaces_an_older_daemon`, `channel::a_newer_daemon_tells_an_old_client_to_update` (cliente por debajo de la ventana), `methods::tests::protocol_5_methods_are_new_and_not_for_mcp` |
| MCP sin lo nuevo (SEC-12) | `mcp_connections_do_not_get_scopes_or_locate` |
| L-06 | `a_client_refuses_an_open_socket_folder` |
| Repo intacto | `scopes_and_locate_leave_the_repo_intact` (huella del testkit) |
| CLI | `apps/cli/tests/channel_process.rs::daemon_status_says_who_you_act_as` |

**Verificación (2026-10-05, macOS)**: `cargo clippy --workspace --all-targets -- -D warnings` y `cargo test --workspace` en verde; `channel_scopes` cinco veces seguidas sin fallos.

### 6.3 Pendientes de la enmienda

- Linux y Windows: *Pendiente: etapa de validación multiplataforma*. Las pruebas de `channel_scopes` son solo de macOS, como las de `channel.rs`. En Windows el canal sigue sin transporte (fail-closed). El clippy cruzado a `x86_64-pc-windows-msvc` no se pudo correr en este Mac (falta el compilador C de `libsqlite3-sys`); lo cubre el CI.
- **INF-CKP-001**: sacar la biblioteca cliente de `crates/core` (A1, V5).
- N8, N9, N10; emisores de `repo.attention`; `autostart` real (US-GRP-004).
