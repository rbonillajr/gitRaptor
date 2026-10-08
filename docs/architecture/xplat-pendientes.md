---
title: Pendientes de la etapa de validación multiplataforma
status: expanded
generated: 2026-10-05
updated: 2026-10-08
generator: orquestador
domain: GRP
tags: [xplat, validacion-multiplataforma, linux, windows, contenedor, vm, lima, utm, ssh, repo-intact, strace]
related: [NFR-01, NFR-07, ADR-GRP-005, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRD-008, INF-GRP-001, TS-GRP-004]
---

# Pendientes de la etapa de validación multiplataforma

Índice de todo lo marcado en el repo como **"Pendiente: etapa de validación multiplataforma"**. Rene decidió validar Linux y Windows en una etapa propia, separada de las historias. Este documento **no sustituye** las marcas: cada artefacto de origen conserva la suya, y aquí se agrupan, se asigna el canal de validación y se registra el estado.

- **Conteo (2026-10-05, commit base `bfedf1f`)**: 127 marcas. `grep -rnic "validación multiplataforma" --exclude-dir={node_modules,target,.git,xplat} .` da 124 líneas (102 en 52 archivos de `docs/`, 18 en 17 archivos de código y 4 en `.github/workflows/repo-intact.yml`). Hay 3 más con la frase partida en dos líneas: `crates/core/tests/watch.rs:5`, `crates/core/src/watch/mod.rs:57` y `crates/core/src/channel/peer.rs:153`. Se agrupan en 29 pendientes (XP-01 a XP-29).
- **Canales**:
  - **Contenedor**: `xplat/run-linux.sh`, con Ubuntu 24.04 arm64 en Docker Desktop.
  - **VM Linux**: Lima o UTM, según [`xplat/linux/vm.md`](../../xplat/linux/vm.md).
  - **Máquina Windows**: la real, por SSH (receta [abajo](#máquina-windows-real)).
  - **CI**: GitHub Actions (`repo-intact.yml`).
- **Estados**: `pendiente` (sin validar), `pasa` (validado y en verde; dice dónde y cuándo), `falla` (validado con un fallo abierto, con su ficha), `parcial` (validado solo en parte; dice qué falta) y `no verificado` (se intentó, pero no se pudo comprobar).

## Pendientes

Las rutas cortas de la columna Origen van bajo `docs/requirements/features/`. La lista completa de archivo:línea por pendiente está en el [anexo](#anexo-marcas-por-pendiente).

| ID | SO | Origen | Qué validar | Canal | Estado |
|---|---|---|---|---|---|
| XP-01 | Windows | TS-GRP-004, ADR-GRP-005 § 5, ADR-CKP-003, SEC-08 | Canal local por named pipe: DACL con el SID del usuario, primera instancia, rechazo de clientes remotos, SQOS e identidad del servidor | Máquina Windows | hecho (2026-10-07, ver "Canal por named pipe" y DS-TS-GRP-004 § 8); falta probar un cliente de otra cuenta real de Windows. Los comandos reservados se aceptan desde la consola de la persona desde el 2026-10-08 (XP-34, TQ-14) |
| XP-02 | Windows | TS-GRP-004 | Reloj monotónico con QPC comparable entre procesos | Máquina Windows | pendiente |
| XP-03 | Linux | TS-GRP-004 | Identidad del par con `SO_PEERCRED` y `/proc` (inicio calculado a 100 Hz; falta `SO_PEERPIDFD`) | Contenedor | parcial: los tests del canal con daemon en proceso pasan en Linux (2026-10-08, "Ronda Linux"). Se corrigió que el inicio de `/proc` se comparaba con el reloj de pared. Falta `SO_PEERPIDFD` |
| XP-04 | Linux | US-GRP-001, US-GRP-002, US-GRP-012, TS-GRP-004, INF-GRP-001 | Llevar a Linux los e2e de proceso que hoy son `cfg(target_os = "macos")` (`script`, `nc -U`, `lsof`) | Contenedor | parcial: `apps/cli/tests/daemon_process.rs` pasa (2026-10-05). Desde el 2026-10-08 ("Ronda Linux") también corren y pasan en Linux los de daemon en proceso de `crates/core/tests`: `channel`, `channel_protected`, `channel_scopes`, `channel_capabilities`, `us_tmc_001`, `us_tmc_002`, `us_tmc_004`, `daemon_tiers` y `discovery`. Siguen sin portar `channel_process`, `protected_process`, `repo_state`, `base_branch`, `live_changes` y `mcp/on_demand` |
| XP-05 | Linux + Windows | US-GRP-002 (D2) | Watcher compartido: `max_user_instances`, agotamiento de `max_user_watches` y handles de Windows al borrar un worktree | VM Linux (Lima) + máquina Windows | parcial: `crates/core/tests/watch.rs` pasa en el contenedor (2026-10-05); los límites del kernel quedan para Lima |
| XP-06 | Linux + Windows | ADR-GRP-011, INF-GRP-002, SPIKE-GRP-002, NFR-04 | Presupuesto de frescura (≤ 300 ms p95), `timer_slack` y tiempos del observador. **Decisión de Rene (2026-10-07)**: en los runners compartidos el banco solo avisa; el presupuesto absoluto se mide en una máquina de referencia | VM Linux (Lima, como referencia) + máquina Windows | pendiente |
| XP-07 | Linux | INF-GRP-001, ADR-GRP-009 (Validación 7), NFR-01 | Auditoría de `exec` con `strace -f` sin root y suites `repo_intact` en verde | Contenedor + CI `ubuntu-latest` | **pasa** en el contenedor (2026-10-05): 72 tests `repo_intact` con `GITRAPTOR_EXEC_AUDIT=strace`, incluido `kernel_tracer` |
| XP-08 | Windows | INF-GRP-001, TS-GRP-002 | Git en fail-closed (falta el chequeo de ACE), canario `.exe`, ETW o Job Object, daemon fsmonitor y paso no bloqueante del CI | Máquina Windows | pendiente |
| XP-09 | Linux | INF-GRP-001 | Repo de otro uid real (segundo usuario) | Contenedor (con un usuario más) | pendiente |
| XP-10 | Varios | INF-GRP-001 (D11) | `repo-intact.yml` ejecutado en los tres SO | CI | pendiente: se verifica con el CI de este PR |
| XP-11 | Linux | TS-TMC-001 | Almacén de Time Machine: `FICLONE`, respaldo por copia y `fsync` | Contenedor; reflink real en VM (btrfs o xfs) | parcial: los tests `tm_store_*` pasan en overlayfs (2026-10-05), donde `FICLONE` cae al respaldo por copia; el reflink sigue sin probar |
| XP-12 | Windows | TS-TMC-001, TS-TMC-003, ADR-TMC-001 | Almacén y escritura (hoy `Unsupported`): `ReplaceFileW`, `FILE_FLAG_OPEN_REPARSE_POINT`, archivo abierto en un editor | Máquina Windows | **hecho** (2026-10-08, ver "Time Machine en Windows" y DS-TS-TMC-003, Enmienda 2026-10-08). Queda pendiente el e2e con el binario real (`raptor undo` por el canal), que en Windows rechaza al solicitante sin identidad verificable (XP-19 y TQ-14). También quedan las anotaciones de locks para la recuperación (XP-15) y el suelo de espacio libre. |
| XP-13 | Linux | TS-TMC-003 | Aplicador con `renameat2` (`EXCHANGE` y `NOREPLACE`) | Contenedor | **pasa** (2026-10-05): `tm_apply` en verde con Git 2.38.5, 2.43.0 y 2.56.0 |
| XP-14 | Linux | TS-TMC-002 | Oplog con rustix | Contenedor | **pasa** (2026-10-05) |
| XP-15 | Windows | TS-TMC-002 | Identidad estable de archivo para los locks y `SystemProbe` (hoy da por vivo cualquier proceso) | Máquina Windows | **hecho** (2026-10-08, ver "Locks y procesos en Windows (XP-15)" y DS-TS-TMC-002, Enmienda 2026-10-08). Solo en NTFS: en FAT, exFAT y ReFS el lock se conserva como `unsupported`. |
| XP-16 | Linux | TS-TMC-004 | Procesos del usuario leídos de `/proc` para el multiplexor | Contenedor | pendiente: no lo ejercita ningún test actual |
| XP-17 | Linux | TS-TMC-004, ADR-GRP-005 § 6, ADR-CKP-002, TS-CKP-002, ADR-MCP-001, ADR-GRD-007 | Daemon como subreaper (`PR_SET_CHILD_SUBREAPER`) frente a un descendiente con doble fork | Contenedor (el daemon no es PID 1) | pendiente: no lo ejercita ningún test actual |
| XP-18 | Linux | ADR-CKP-002, TS-CKP-002, TS-CKP-003 | Ejecutor: el hijo muere con el daemon, identidad del hijo antes del `exec` y variables de sesión | VM Linux (UTM, sesión gráfica) | pendiente |
| XP-19 | Windows | ADR-TMC-005 § 3, ADR-CKP-002, ADR-GRP-005, US-CKP-020, TS-CKP-002, TS-CKP-003, TS-TMC-004 | Rechazo de trabajo ajeno, capa `cockpit`, barrera, consola y `CREATE_NO_WINDOW`, e identidad después del `exec` | Máquina Windows | pendiente |
| XP-20 | Linux + Windows | ADR-CKP-001, SPIKE-CKP-001, TS-CKP-001 | Predictor de conflictos: prioridad baja del SO, latencia ≤ 5 s p95 y auditoría de `exec` | VM Linux (Lima) + máquina Windows | pendiente |
| XP-21 | Linux + Windows | ADR-CKP-003, INF-CKP-001, TS-CKP-004, US-CKP-005, NFR-09, design system | TUI en terminales reales: ancho de `⚡⛔⚠ℹ`, crossterm en la consola de Windows y señales | VM Linux (UTM) + máquina Windows | pendiente |
| XP-22 | Linux + Windows | US-CKP-013, ADR-CKP-003 | Abrir en el editor externo, gráfico y de terminal | VM Linux (UTM) + máquina Windows | pendiente |
| XP-23 | Linux | seq-ckp-arranque-tui | Arranque del daemon como servicio de usuario | VM Linux (Lima, `systemd --user`) | pendiente |
| XP-24 | Varios | ADR-MCP-001 (Validación 2, 3, 8, 11 y 12), INF-MCP-001, US-MCP-001, US-MCP-003, US-CKP-019, NFR-02 | cwd de otro proceso, ascendencia, identidad, CLI `claude` y ámbito del MCP fuera de macOS | Contenedor (Linux) + máquina Windows | pendiente |
| XP-25 | Windows | US-MCP-010, US-CKP-018, INF-MCP-001 | Rutas UNC y enlaces en el corpus de seguridad | Máquina Windows | pendiente |
| XP-26 | Linux + Windows | SPIKE-GRD-002, ADR-GRD-008, US-GRD-013, US-GRD-015 | Factor del SO llamado por el daemon (polkit, Windows Hello), con fail-closed mientras no exista | VM Linux (UTM) + máquina Windows | pendiente (lo lleva ADR-GRD-008, en otra sesión) |
| XP-27 | Linux + Windows | INF-GRP-003 | Binarios de release probados más allá de `--version` | Contenedor + máquina Windows | pendiente |
| XP-28 | Windows | INF-GRP-003, ADR-GRP-014 | Artifact Signing en Windows ARM (`windows-11-arm`) | Máquina Windows ARM | pendiente |
| XP-29 | Windows | INF-GRP-004, ADR-GRP-014, ADR-GRP-005 § 4 | Actualizar con winget o `install.ps1` mientras corre el daemon | Máquina Windows | pendiente |
| XP-30 | Varios | INF-GRD-001, ADR-GRD-002 (Validación 12 y 13), SPIKE-GRD-001 (D11) | Datos por versión de Git de la matriz de interceptabilidad: la fila de `rename-over-base` en reftable para Git ≥ 2.56.0 (publicada C; este ejecutor mide B) y las filas de `COST_TABLE` para Git 2.56.0 (y 2.43.0, el Git de la distro del contenedor). No depende del SO: Windows y el contenedor Linux miden lo mismo con 2.56.0 | Máquina Windows + contenedor | **pasa** en el contenedor (2026-10-06, XP-30): `./xplat/run-linux.sh` en verde con Git 2.43.0, 2.38.5 y 2.56.0. No era un cambio de Git 2.56: la fila C venía de la sonda del spike (ADR-GRD-002, Enmienda 2026-10-06). Se añadieron las filas de procesos de hook de 2.38.5, 2.43.0 y 2.56.0. Windows real: pendiente de volver a pasar en la máquina de Rene |
| XP-31 | Linux | US-GRD-001, ADR-GRD-001 Validación 13 | Suites de US-GRD-001 (escenarios, criterios y recuperación) con la terminal del desarrollador vía `script` de util-linux, con Git 2.38.5 y la distro (2.43) | Contenedor + CI (`guardrails-git-min`) | **pasa** en el contenedor (2026-10-05): 22/22 con Git 2.38.5 y con 2.43.0 (arm64). Corrección (2026-10-06, XP-30): hasta entonces el daemon de esas suites usaba siempre `/usr/bin/git` (2.43). Desde XP-30 usa el Git de la etapa (`GITRAPTOR_TEST_GIT`) y pasa con 2.43.0, 2.38.5 y 2.56.0. La matriz de SPIKE-GRD-001 en Linux sigue pendiente |
| XP-32 | Windows | US-GRD-001, SPIKE-GRD-001 § 11 y § 14, ADR-GRD-001 (Enmienda 2026-10-05) | Coste del dispatcher (`sh` frente a nativo) y dispatcher nativo en modo degradado; la instalación espera al canal (XP-01) y la DACL de la carpeta (M-07) | Máquina Windows | parcial: coste medido (2026-10-05, § 14 de los resultados del spike); ver el PR de US-GRD-001 para el humo funcional |
| XP-33 | Windows | INF-TMC-001, US-TMC-019, ADR-TMC-003 § 6 | Arnés de caos (`apps/cli/tests/tm_chaos.rs`): muerte del daemon en cada punto de fallo de un undo y escenarios hostiles de Git, con `raptor undo` real. Solo macOS y Linux: el CI de Windows no corre tests con Git (TS-GRP-002) y no hay `SIGKILL` (los puntos usan `abort`) | Máquina Windows + CI | pendiente |
| XP-34 | Windows | TS-GRP-004 (DS § 9, TQ-14), ADR-GRP-005 § 6 (Enmienda 2026-10-08), TS-GRP-007 | Prueba de presencia por consola interactiva para los comandos reservados (opción A de TQ-14) | Máquina Windows | **hecho** (2026-10-08, ver "TQ-14 en Windows"). Falta Windows 11 con Windows Terminal como terminal por defecto y la terminal de VS Code (ConPTY), que pueden quedar rechazados (fail-closed), y que Rene lo confirme tecleando en su propia consola |

## Contenedor Linux: resultados del 2026-10-05

`xplat/run-linux.sh` sobre el commit de este PR, con Ubuntu 24.04 arm64, kernel 6.12.76-linuxkit, rustc 1.99.0, strace 6.8, Node 22.23.3 y pnpm 10.20.0. Corre como usuario sin privilegios, con el repo clonado desde un bundle en el filesystem del contenedor.

| Etapa | Pasan | Fallan | Ignorados |
|---|---|---|---|
| `pnpm install --frozen-lockfile` | ok | — | — |
| `cargo test --workspace` (Git 2.43.0 de la distro) | 448 | 0 | 2 |
| `cargo test --workspace -- repo_intact` con `GITRAPTOR_EXEC_AUDIT=strace` | 72 | 0 | 0 |
| `cargo test --workspace` con Git 2.38.5 (mínimo, NFR-07) | 448 | 0 | 2 |
| `cargo test --workspace` con Git 2.56.0 (última) | 448 | 0 | 2 |

Los 2 ignorados lo están por diseño: `m1_child_reads` lo lanza `gitoxide_never_launches_git` y `budget_warm_load_p95` es de tiempos y se corre a mano en release.

### Fallos de la primera pasada y su clasificación

> **Decisión del orquestador (2026-10-04), validada por Arquitecto**: hay cinco clases de fallo.
>
> - **(a) Bug real de Linux**: se corrige en el PR si es acotado; si no, se abre una ficha TD.
> - **(b) Límite del contenedor**: pasa a la VM.
> - **(c) Test frágil**: solo con un criterio reproducible (20 repeticiones con su resultado) y con ficha TD.
> - **(d) Supuesto del test o del arnés que solo vale en otro SO o en otra versión de Git.**
> - **(e) Fallo del entorno**: build de la imagen o red.
>
> Ningún test se relaja.

| Fallo | Dónde | Clase | Causa | Resolución |
|---|---|---|---|---|
| `repo_intact::sensitivity_lock_created_and_deleted_is_detected` | `crates/testkit/tests/harness.rs`, con todas las versiones de Git | (d), parecía (c) | Linux pone mtime y ctime con un reloj de grano grueso: un tick, 1 ms con `CONFIG_HZ=1000`. Si un `.git/index.lock` se crea y se borra en el mismo tick que el último cambio del fixture, el mtime del directorio no cambia y el arnés no ve la escritura. macOS no lo muestra porque APFS registra en nanosegundos. | Corregido: `wait_for_timestamp_tick()` en `crates/testkit/src/fingerprint.rs`, llamado tras el snapshot "antes" de `check` y de `Scenario`. Antes fallaba 20 de 60 repeticiones (20 por Git); ahora 0 de 120. El test no se tocó. |
| `repo_intact::control_gc_auto_by_agent_commit_is_not_imputed` | El mismo archivo, solo con Git 2.56.0 | (d) | Git 2.56 corre `git maintenance run --auto` con la estrategia geométrica por defecto, que ignora `gc.auto`, así que el escenario de control nunca reempaquetaba. | Corregido: el fixture fija también `maintenance.strategy=gc`, que Git 2.38 y 2.43 ignoran (comprobado). Las aserciones siguen igual. 0 fallos en 60 repeticiones con las tres versiones. |
| Build de la imagen: `cargo: not found` al compilar Git 2.56 | `xplat/linux/Dockerfile` | (e) | Git reciente compila por defecto sus partes en Rust (`NO_RUST` las desactiva). | Se instala una toolchain temporal para el build, igual que lo publica upstream. |

No apareció ningún bug real de Linux en el código de producto (clase a) ni ningún límite del contenedor en las suites automáticas (clase b). Los límites conocidos de antemano ya están asignados a VM en la tabla.

**Cobertura de arquitectura**: el contenedor solo prueba **arm64**. En x86_64, el CI (`ubuntu-latest`) prueba solo el Git de la distro, así que el mínimo 2.38.5 está validado únicamente en arm64. El comportamiento de Git no depende de la arquitectura.

## Ronda Linux (2026-10-08)

Validación en el contenedor de todo lo mergeado desde el 2026-10-06 con código específico por plataforma. Hasta ahora, en Linux solo lo había probado el CI de ubuntu. Incluye la observación por niveles con centinela (TS-GRP-006, #164 y #167), los repos descubiertos con watcher inotify no recursivo (US-GRP-020, #170), la segunda línea frente a `--no-verify` con `/proc/<pid>/cmdline` (#141), el barrido de temporales (#172), la atribución S4 (#155), el registro de bloqueos (#154), el MCP (#140, #157 y #159) y el cliente del canal en `crates/api` (#174).

Entorno: Ubuntu 24.04 arm64, kernel 6.12.76-linuxkit, rustc 1.99.0, strace 6.8, `fs.inotify.max_user_watches` = 1048576. Docker Desktop con 7,6 GB. Rama `fix/linux-validation-round`.

| Pasada | Commit | Etapa | Pasan | Fallan | Ignorados |
|---|---|---|---|---|---|
| Línea base (`main`) | `42b7d1d` | Git 2.43.0 (distro) | 1074 | 0 | 6 |
| Línea base (`main`) | `42b7d1d` | Git 2.38.5 | 1074 | 0 | 6 |
| Línea base (`main`) | `42b7d1d` | Git 2.56.0 | 1074 | 0 | 6 |
| Línea base (`main`) | `42b7d1d` | `repo_intact` con `GITRAPTOR_EXEC_AUDIT=strace` | 129 | 0 | 0 |
| Tests de daemon en proceso habilitados en Linux, sin arreglos | `42b7d1d` + 9 `cfg` | Git 2.43.0 | 1167 | 4 | 6 |
| Después de esta rama | esta rama | Git 2.43.0 (distro) | 1174 | 0 | 7 |
| Después de esta rama | esta rama | Git 2.38.5 | 1174 | 0 | 7 |
| Después de esta rama | esta rama | Git 2.56.0 | 1174 | 0 | 7 |
| Después de esta rama | esta rama | `repo_intact` con `GITRAPTOR_EXEC_AUDIT=strace` | 130 | 0 | 0 |

Los 7 ignorados lo están por diseño:

- `m1_child_reads`, `sweep_cycles_under_a_git_shim` y el nuevo `raw_child_asks_to_stop`: los lanza otro test.
- `budget_warm_load_p95` y `latency_report`: son de tiempos y se corren a mano.
- `a_user_scope_server_starts_in_the_session_folder` e `install_and_uninstall_with_the_real_claude_code`: necesitan la CLI de Claude Code.

Además se pasó `cargo clippy --workspace --all-targets -- -D warnings` dentro del contenedor, porque el código `cfg(target_os = "linux")` no se lintea en macOS. Salió limpio. Los tests de esta rama se repitieron 20 veces en el contenedor sin ningún fallo: los de `channel_protected` y `channel` con un hijo del daemon, `discovery_*`, los de dormidos de `daemon_tiers` y `channel::peer::linux_tests`.

**Hallazgo principal**: la línea base salía en verde, pero no cubría lo que se pedía validar. `crates/core/tests/discovery.rs`, `daemon_tiers.rs`, los cuatro `channel*.rs` y los tres `us_tmc_00*.rs` eran `#![cfg(target_os = "macos")]`. En Linux no se ejercitaba el inotify de discovery, el centinela de los dormidos ni la identidad del par en el canal. Esta rama los habilita en Linux, y en total pasan a correr 93 tests más. Al habilitarlos salieron 4 fallos:

| Fallo | Clase | Causa | Arreglo |
|---|---|---|---|
| `channel_protected::a_child_of_the_operation_cannot_use_a_reserved_command` (`via: None` en lugar de `Executor`) | (a) | En Linux, `ProcInfo.start_us` se calcula con `btime` (segundos enteros) más los ticks de 100 Hz desde el arranque, pero se comparaba con instantes del reloj de pared: `opened_us` de la marca del ejecutor y `accepted_us` del par. El truncado de `btime` adelanta hasta 1 s el inicio calculado, así que un nieto de la operación, con doble fork, salía "anterior" a la marca y perdía su atribución al ejecutor (DEP-MCP-3, H-01). | `channel::peer::proc_clock_us()` da el "ahora" en el mismo reloj que `start_us`: en Linux, `btime` más `/proc/uptime` en ticks enteros, y en el resto de SO el reloj de pared. Lo usan la apertura de la marca (`timemachine::protected`) y el `accepted_us` del canal. El test unitario nuevo `a_child_started_after_the_clock_is_not_older` (20 hijos) fija la propiedad. |
| `daemon_tiers::a_dormant_repo_has_its_store_closed` (`NotFound`) | (d) | El test usaba `lsof`, que no hay en la imagen ni en una instalación mínima. | En Linux lee `/proc/self/fd` y `/proc/self/maps`, igual que lo que lista `lsof`. La aserción no cambia. |
| `discovery::discovery_the_home_root_skips_hidden_and_excluded_folders` | (d) | El test esperaba las exclusiones de macOS (`Documents`). En Linux, la exclusión de la carpeta personal es `snap`. | El test crea también `~/snap` y espera las exclusiones de cada plataforma: en macOS se excluye `Documents` y se propone `snap`; en Linux al revés. Así se valida la exclusión `snap`. |
| `channel::a_child_of_the_daemon_cannot_use_reserved_commands` (salida vacía) | (d) | El hijo del daemon era `sh` con `/usr/bin/nc -U`, y `nc` no está en la imagen. | El hijo es una copia del binario de test (`raw_child_asks_to_stop`, ignorado y lanzado por el test), que habla JSON-RPC por el socket sin la librería cliente. Las aserciones no cambian, y en macOS también pasa. |

**Comprobaciones pedidas**:

- **Inotify no recursivo de discovery**: hay un test nuevo, `discovery_a_normal_root_is_listed_on_a_change_of_its_first_level` (Linux y Windows), con todos los intervalos a 1 h menos el `settle`. Un repo creado después del primer listado solo puede llegar por el watch, y llega. La raíz amplia nunca se vigila: no hay watch y se lista cada `broad_poll`, lo que cubren los tests de raíz `home`. La exclusión `snap` queda validada (ver la tabla).
  - **Observación, no es un fallo**: una carpeta creada primero y convertida en repo después (`mkdir x` y, pasados unos segundos, `git init` dentro) no genera ningún evento en la raíz, porque el watch no es recursivo. Se propone en el listado de seguridad de 5 min. Lo comprobó un experimento en el contenedor, que no se conserva como test. Está dentro de lo que acepta ADR-GRP-010 N6 ("el listado de seguridad … cubre los eventos perdidos y los clones en curso"). En macOS el sondeo es de 30 s. Si molesta en el dogfooding, se puede vigilar un rato las carpetas nuevas del primer nivel.
- **Centinela de los dormidos con inotify**: `daemon_tiers` pasa 8 de 8 en Linux, con las tres versiones de Git. Incluye `edit_then_reset_hard_in_a_dormant_repo_is_recoverable` (la condición NFR-01 del PO, Q49), `commits_while_dormant_reach_the_history_in_order` y `a_dormant_repo_has_its_store_closed`. También pasan `observe_tiers` (`the_sweep_finds_a_commit_the_sentinel_missed` y `the_slow_reconcile_finds_an_edit_the_sentinel_missed`).
- **`/proc/<pid>/cmdline` en la segunda línea (#141)**: los tests de `second_line` y el e2e `guard_us_grd_018` pasan en Linux con las tres versiones de Git.
- **`ETXTBSY`**: ninguno en 5 pasadas completas (unas 5.800 ejecuciones de test). Aun así, `testkit::canary::script` escribía el script con un descriptor propio, el patrón que corrigió #152, y lo usan las suites `repo_intact`, con muchos hilos lanzando Git. Ahora lo escribe un `sh` hijo. Hay otros sitios con el mismo patrón que no se tocan en esta rama (riesgo latente, sin fallo observado):
  - `crates/core/tests/watch.rs:471`, `observe_tiers.rs:394` y `channel_protected.rs:1219`.
  - `apps/cli/tests/` (`daemon_process`, `raw_git_undo`, `live_fleet`, `continuous_observation`, `events_us_grd_019`, `claude_sessions`, `repo_state` y `guard_us_grd_001`).
- **Atribución S4 (#155)**: pasa. `detect` compara el inicio de `/proc` con el reloj de pared con 1 s de tolerancia (`start_tolerance`), así que es inclusivo por diseño. No se cambia aquí, pero ahora podría usar `proc_clock_us` y quitar la tolerancia.
- **Barrido de temporales (#172), registro de bloqueos (#154), MCP (#140, #157 y #159) y cliente del canal en `crates/api` (#174)**: sus tests pasan en Linux con las tres versiones de Git, y no hubo fallos.

**Arreglos del arnés** (clase (e)):

- `xplat/run-linux.sh --dirty` en macOS metía archivos AppleDouble (`._<nombre>.rs`) por los atributos extendidos, y los tests de frontera, que leen todos los `.rs`, fallaban con UTF-8 inválido. Ahora el script usa `COPYFILE_DISABLE=1`.
- Con 7,6 GB en Docker, `ld` moría por memoria (`signal 9`) al reenlazar todos los binarios de test. El script ahora pasa `CARGO_BUILD_JOBS` al contenedor cuando está definido; esta ronda usó 3.

**Sigue pendiente**: los límites del kernel (`max_user_watches` y `max_user_instances`, XP-05) en Lima; `SO_PEERPIDFD` (XP-03); los e2e de proceso que siguen siendo solo de macOS (XP-04); y x86_64, que solo cubre el CI de ubuntu.

## Máquina Windows real

> Receta de la máquina que preparó el coordinador para validar Windows por SSH (`ssh gitraptor-win`). Se documenta a partir de su descripción: este PR no la ejecutó ni la verificó.

1. **OpenSSH Server**: es la capacidad opcional de Windows (`Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0`), con el servicio `sshd` en arranque automático y la regla de firewall del puerto 22. En el Mac, un alias `gitraptor-win` en `~/.ssh/config` con clave pública. Para usuarios administradores, la clave va en `C:\ProgramData\ssh\administrators_authorized_keys`, no en el perfil.
2. **Git for Windows**: instalación estándar en `C:\Program Files\Git`. Es una ubicación conocida del resolvedor (`%ProgramFiles%\Git\cmd\git.exe`, ver `crates/git/src/resolve.rs`).
3. **rustup con la toolchain MSVC** (`x86_64-pc-windows-msvc` o `aarch64-pc-windows-msvc`, según la máquina). Al entrar en el repo, `rust-toolchain.toml` instala la versión fijada.
4. **Visual Studio Build Tools** (workload *Desktop development with C++*: MSVC y Windows SDK) como **tarea programada que corre como SYSTEM**. Ni `winget` ni el instalador de Visual Studio funcionan en una sesión SSH, porque no hay escritorio interactivo y el bootstrapper no avanza. Se registra una tarea con `schtasks /create /ru SYSTEM /sc once ...` (o `Register-ScheduledTask`) que lanza `vs_BuildTools.exe --quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`, se ejecuta con `schtasks /run` y se espera a que termine. Después hay que comprobar que `link.exe` existe y que `cargo build` enlaza.
5. **gh** (GitHub CLI), para clonar ramas y consultar los checks del PR desde la máquina.

**Flujo**: clonar la rama dentro de la máquina; no se comparte el directorio del Mac. Después, `cargo test --workspace` y `cargo test --workspace -- repo_intact` (la auditoría por trampas; ETW sigue pendiente, XP-08), y el resultado se anota en la columna Estado.

### Segunda ronda (2026-10-05)

`cargo test --workspace --no-fail-fast` en la máquina real (Windows 10 Pro 19045, Git for Windows 2.56.0, toolchain MSVC), en un clon nuevo de la rama `fix/windows-round2`.

| Pasada | Commit | Pasan | Fallan | Ignorados |
|---|---|---|---|---|
| Línea base del coordinador (checkout antiguo de `main`) | `e4dbe16` | 474 | 22 | 2 |
| Línea base en un clon nuevo | `e4dbe16` | 475 | 21 | 2 |
| Después de esta rama | `251a295` | 496 | 3 | 2 |

| Fallo | Clase | Causa | Resolución |
|---|---|---|---|
| `settings::schema::tests::schema_has_not_drifted` | (e) | El checkout del coordinador era anterior a `.gitattributes` y tenía CRLF. | Ninguna: en un clon nuevo pasa. |
| `user_ops_preflight` (4) | (a) | `preflight` canonicalizaba con `std` (forma verbatim `\\?\C:\…`), que la capa de lectura rechaza (SEC-02); y en Windows no había identidad de archivo, así que un `.git` sustituido pasaba inadvertido (M-05). | Corregido: `paths::canonicalize` (forma de unidad, decisión de #74) y la identidad sale del número de serie del volumen y el índice del archivo (`gitraptor-winsys::file_id`, sin seguir enlaces). |
| `user_ops::tests` (3) y el test de `update-ref` | (d) | Rutas Unix (`/usr/bin/git`, `/w/repo`) y el editor de rechazo `/usr/bin/false`, que no es absoluto en Windows. | Tests con rutas de la plataforma. En producción el ejecutor sigue rechazando todo lanzamiento en Windows hasta que exista su editor de rechazo (XP-19, fail-closed), y un test `cfg(windows)` lo fija. |
| `catalog::tests::unknown_operations_and_fields_fail_the_schema` | (d) | `/tmp/x` no es absoluta en Windows. | Ruta absoluta de la plataforma. |
| `daemon::lock` (1) y `daemon_lifecycle` (2) | (a) | `File::try_lock` de std bloquea todo el archivo en Windows, y otro handle no puede leer un rango bloqueado: el PID del dueño nunca se leía y `running_pid` decía "no corre" con el daemon vivo. | Corregido: en Windows el lock cubre un byte en 2^62 (`gitraptor-winsys::file_lock`), así que el PID sigue legible. Un lock tomado sin PID legible ya nunca se informa como libre. |
| `timemachine::protected::backend::tests::the_lock_key_is_the_appliers` | pendiente | El almacén de snapshots no existe en Windows (`Unsupported`). | Marcado `cfg(unix)` con XP-12, como `tm_store_capture` y `tm_store_safety`. |
| `timemachine::protected::tests::a_full_disk_from_the_store_reads_as_no_space` | (d) | El test usaba el código 28 (`ENOSPC`), que en Windows es `ERROR_OUT_OF_PAPER`. | En Windows usa 112 (`ERROR_DISK_FULL`); producción ya mapeaba 39 y 112. |
| `repo_intact::exec_audit::trap_gate_on_reads` | (d) | El shim era un hard link del binario de test, que comparte su DACL: bajo `C:\src` hereda Modify para Authenticated Users, y el resolvedor lo rechaza con razón (SEC-10). | En Windows el shim se copia y hereda la DACL de su carpeta privada. El resolvedor no cambia. |
| `hooks_harness` (5) | (d) | Comparaciones de `core.hooksPath` como cadenas (`/` frente a `\`), el origen entre comillas que Git da a una ruta con `\` y los separadores del informe. | Se comparan como rutas y se normalizan los separadores en las aserciones. |
| `interceptability` (2) | XP-30 | Datos de Git 2.56.0, no de Windows: el contenedor Linux con Git 2.56.0 mide lo mismo (B en `rename-over-base` reftable y conteos de hooks idénticos). | Resuelto en XP-30 (2026-10-06): datos de 2.56.0 corregidos para todos los SO. |
| `watch::commit_merge_rebase_and_push_are_named_by_the_reflog` (solo en la última pasada) | sin clasificar | Durante el `rebase` llegó un evento `BranchSwitch`. Pasó en las otras tres pasadas completas y 15 de 15 veces aislado, con dos workers más compilando en la misma máquina. No lo toca esta rama. | Sin cambio. Si se repite bajo carga, se abre una ficha TD con el criterio de la clase (c); es del watcher (XP-05 y XP-06). |

> **Decisión del orquestador (2026-10-05), validada por Arquitecto**:
>
> - El lock del daemon en Windows cubre un solo byte lejos del PID en lugar de usar un `daemon.pid` aparte. El código con `unsafe` vive en un módulo FFI privado de `gitraptor-winsys` (ADR-GRP-002, Enmienda 2026-10-05). Las versiones viejas y nuevas del daemon siguen chocando, porque el lock de std cubre ese byte.
> - Las comparaciones de contención (rutas de PATH frente a las raíces del repo, M-01, y la ruta de un worktree nuevo frente a las raíces protegidas) usan una sola forma en los dos lados: la de `std::fs::canonicalize`. La forma de unidad se usa solo para la ruta que recibe Git. Así, una ruta que se queda en forma verbatim (un componente acabado en punto) no se escapa de la comparación.
> - El almacén de Time Machine no se porta en esta ronda (XP-12).
> - Las tablas de INF-GRD-001 no se tocan aquí: `cfg(not(windows))` escondería una discrepancia medida y no hay dimensión de SO. Hay que corregir los datos de la versión 2.56.0 para todos los SO.
> - Cuando se use el PID del lock en Windows para terminar un proceso, antes hay que verificar su identidad (imagen y SID) (SEC-06, XP-19).

### Tercera ronda (2026-10-07)

`cargo test --workspace --no-fail-fast` en la misma máquina, en un clon nuevo de la rama `fix/windows-round3` (`C:\src\win-round3`, `CARGO_BUILD_JOBS=2`, con otros dos workers compilando a la vez). Sin `--skip`.

| Pasada | Commit | Pasan | Fallan | Ignorados |
|---|---|---|---|---|
| Línea base (`main`) | `ddc3cfe` | 797 | 4 | 2 |
| Después de esta rama | `dba9607` (lo que viene después en la rama solo cambia código Unix y documentos) | 802 | 0 | 2 |

| Fallo | Clase | Causa | Resolución |
|---|---|---|---|
| `settings::schema::tests::schema_has_not_drifted` (pasadas anteriores) | (e) | Como en la segunda ronda: un checkout anterior a `.gitattributes` (`* text=auto eol=lf`) tenía CRLF. | Ninguna: pasa en el clon nuevo. `.gitattributes` ya fija LF para el schema y los snapshots. |
| `channel::peer::windows_tests::system_is_foreign_and_a_dead_pid_is_gone` (intermitente) | (a) | Un proceso recién terminado que el `Child` del padre aún retiene puede seguir un instante en la lista de Toolhelp: se leía vivo, con `exe: None`. | Corregido en `gitraptor-winsys::process`: si el código de salida ya no es `STILL_ACTIVE`, el proceso es `Gone`. |
| `timemachine::engine::tests::without_a_reflog_the_calm_is_time` | (a) | En Windows el reloj monotónico cuenta desde la primera lectura del proceso (XP-02): justo después del arranque, "nunca tocado" (0) parecía reciente. | Corregido: un repo nunca tocado está en calma, y el reloj de Windows nunca devuelve 0. |
| `clock::tests::never_goes_backwards` | (a) | La primera lectura del reloj de Windows podía ser 0. | La misma corrección. |
| `ctrl_z_suspends_and_repaints` (PR #139) | (d) | El test daba por hecho el control de trabajos de Unix. En Windows `Ctrl-Z` solo avisa (`SuspendUnsupported`) y no toca la terminal: es el diseño (DS INF-CKP-001). | El test pasa a `cfg(unix)`, y `ctrl_z_without_job_control_only_says_so` fija el comportamiento de Windows. |
| `watch::commit_merge_rebase_and_push_are_named_by_the_reflog` (segunda vez bajo carga) | (a) | `read_worktree` leía `HEAD`, luego el status y por último el estado de la operación. Un `rebase` que acaba durante el status se leía como un `HEAD` separado sin operación: la tarea olvidaba su rama y después publicaba un `BranchSwitch` a la misma rama. No es exclusivo de Windows, pero su status lento lo hace visible. | Corregido: el estado de la operación se lee antes y después de `HEAD` (Git crea el estado antes de separar `HEAD` y lo borra después de volver a unirlo). |
| `BrokenPipe` (`io error when listing tests`, os error 232) | (d) | Los tests `requester::real_processes` leían la salida de su copia hasta `CHILD=` y cerraban la tubería; la copia fallaba al imprimir su resumen de libtest. | La tubería se drena hasta el final. |

**Verificación de lo nuevo**:

- **Segunda línea frente a `--no-verify` (PR #141)**: en Windows `SystemProcs::args` es `None` y `evaluates` devuelve `true`: cada commit se evalúa, nunca se salta (fail-closed). Lo fija `second_line::tests::on_windows_the_real_command_line_is_unreadable_and_evaluated` con los procesos reales. El e2e `guard_us_grd_018` es `cfg(unix)` y lo declara (no hay canal en Windows, XP-01); DS-US-GRD-018 ya documenta Windows en § 8 y S2.
- **`Ctrl-Z` de la TUI (PR #139)**: en Windows no se pide la suspensión, no se restaura ni se toma la terminal y se muestra el aviso. Verificado con el test de bucle (`TestBackend`). La TUI en una consola real de Windows sigue en XP-21.
- **MCP `status` (PR #140)**: el daemon canonicalizaba el cwd del par con `std::fs::canonicalize` (`\\?\C:\…` en Windows) y lo comparaba con raíces de worktree en forma de unidad (`observe.rs` usa `gitraptor_git::paths::canonicalize`): en Windows no habría coincidido nunca. Corregido en `mcp.status`, en el ámbito del snapshot por MCP y en `repo.locate`, que ahora usan la misma forma que las raíces (sin cambios en Unix). Es la regla de la segunda ronda, una sola forma en los dos lados de una comparación de contención; aquí manda la forma en que ya se guardan las raíces observadas. Hoy no se puede ejercitar en Windows: no hay canal (XP-01) y `process_cwd` es `None` (XP-24), así que el MCP rechaza sin datos.
- **Exclusiones de las raíces de descubrimiento (PR #145)**: solo están en documentos; el `$HOME` de la máquina se anota en el PR.

### Canal por named pipe (XP-01, 2026-10-07)

El canal ya existe en Windows (DS-TS-GRP-004 § 8). Verificado en la máquina real con un perfil temporal (`GITRAPTOR_PROFILE_DIR`):

- `raptor daemon status` arranca el motor bajo demanda y responde por `\\.\pipe\gitraptor-<SID>-<huella>` (Git 2.56 encontrado); `raptor status` y `raptor events` responden por el pipe.
- Tests nuevos en verde: `winsys` `pipe::tests` (8: DACL real con una sola ACE del usuario, nombre ocupado falla cerrado, ida y vuelta con PIDs, plazo de lectura, `shutdown`, waker, pipe inexistente, tope de instancias) y `crates/core/tests/channel_windows.rs` (5: saludo, llamadas y eventos; squatting hace fallar cerrado al daemon; un pipe con DACL ajena se rechaza con `ChannelRejected`; sin daemon es `NotRunning`; las conexiones por encima del límite reciben `LIMIT_REACHED` y el pipe sigue usable).
- Encontrado al probar: el daemon lanzado bajo demanda heredaba las tuberías estándar del cliente (`raptor daemon status | Out-String` no terminaba nunca) y no encontraba Git porque su entorno limpio no tenía `ProgramFiles`. Corregidos: los handles estándar del cliente dejan de ser heredables antes del lanzamiento, y el entorno lleva `ProgramFiles`, `USERPROFILE` y `LOCALAPPDATA` leídos del sistema, nunca del cliente.
- Sigue pendiente: `raptor daemon stop` se rechaza (`unsupported`) porque en Windows todo comando reservado se rechaza sin prueba de terminal (TQ-14, W1; resuelto en XP-34, 2026-10-08); un cliente de otra cuenta real de Windows no se probó (no hay segunda cuenta en la máquina); `file_id` sigue sin implementarse en Windows, así que `daemon.replace` y la identidad del ejecutable del hook quedan como desconocidas (resuelto en XP-15, 2026-10-08).

### Time Machine en Windows (XP-12, 2026-10-08)

`cargo test --workspace --no-fail-fast` en la máquina real, en un clon nuevo de la rama `feat/XP-12-windows-snapshot-store` (`C:\src\xp12`, `CARGO_BUILD_JOBS=2`, con otros workers compilando a la vez).

| Pasada | Commit | Pasan | Fallan | Ignorados |
|---|---|---|---|---|
| Línea base (`main`) | `fc2f515` | 897 | 0 | 3 |
| Después de esta rama | `ab841ce` (lo que viene después solo cambia imports de tests y documentos; `cargo clippy --all-targets -- -D warnings` limpio en la máquina) | 956 | 0 | 3 |

Antes de esta rama se saltaban en Windows estos tests, que ahora corren y pasan: `tm_store_capture` (19), `tm_store_safety` (9, incluido el nuevo de la junction), `tm_apply` (17) y `tm_write_maintenance` (1). También pasan `protected::backend::tests::the_lock_key_is_the_appliers` y los tests nuevos `cfg(windows)`: `files::windows::tests` (7), `tm_write_windows` (1), `winsys` `fs::tests` (2) y `file_id` (1).

**Decisiones (DS-TS-TMC-003, Enmienda 2026-10-08, W1–W8)**:

- El reemplazo hace dos renombrados exclusivos (`MoveFileExW` sin `REPLACE_EXISTING`) y compara antes de entrar. No se usa `ReplaceFileW`.
- Las carpetas del camino quedan fijadas con handles abiertos sin `FILE_SHARE_DELETE`.
- Lo desplazado se mantiene abierto, compartido solo para lectura, mientras se compara y se borra por ese mismo handle.
- Todos los nombres se toman en forma `\\?\`.
- Los reparse points nunca se siguen.
- El bit ejecutable se ignora, y los enlaces se escriben como archivos, igual que Git for Windows con `core.symlinks=false`.
- Un archivo abierto por otro programa se reintenta durante unos 1,5 s y después da `Locked`: la operación queda interrumpida y se recupera con undo.

**Encontrado al probar**:

- `FlushFileBuffers` necesita un handle con escritura: con un handle de solo lectura daba "acceso denegado" en cada objeto del almacén.
- Git no lee una configuración en una ruta `\\?\`, así que las raíces van siempre en forma de unidad.
- El caché de stat de la captura no veía una escritura que conservaba el tamaño y la fecha de modificación (NTFS no tiene inodo). Ahora usa el `ChangeTime` de NTFS, que ninguna herramienta puede retrasar.

**Sigue pendiente**:

- El e2e del binario real (`undo_process`, `raw_git_undo`): el daemon de Windows rechaza al cliente con "the caller's identity could not be verified" (TQ-14, XP-19). El mismo flujo se verifica a nivel de motor con `work_thrown_away_by_a_raw_reset_hard_comes_back` y `a_file_open_in_an_editor_interrupts_and_undo_recovers_once_closed`.
- El barrido de temporales `.gitraptor-tm-*` tras un crash (INF-TMC-001).
- Los marcadores de la nube (OneDrive), que no se rechazan en las precondiciones.
- Los archivos comprimidos con `compact.exe` dan solape al restaurarlos.
- Medir el coste de `FlushFileBuffers` por objeto (ADR-TMC-006).

### Locks y procesos en Windows (XP-15, 2026-10-08)

**Qué cambia** (DS-TS-TMC-002, Enmienda 2026-10-08):

- **Identidad de los locks del oplog**: índice de archivo de NTFS (incluye el número de secuencia del registro MFT, así que un registro reutilizado da otro índice) y `CreationTime`. La identidad se lee siempre por handle, sin seguir enlaces ni junctions.
- **Liberación**: se abre el lock por su ruta, sin seguir reparse points y con acceso `DELETE`. Solo se borra si es un archivo regular en NTFS con el índice y la hora anotados, y se borra por ese mismo handle.
- **Otros sistemas de archivos**: en FAT y exFAT el índice es la posición de la entrada en la carpeta y se reutiliza; en ReFS el índice de 64 bits no es único. En esos casos el lock se conserva como `unsupported`.
- **`SystemProbe` en Windows**: la entrada `child-started` guarda la hora de inicio del hijo (`start_us`, la misma que lee el canal). Se toma por vivo solo el proceso con ese PID y esa hora exacta: un PID muerto o reutilizado no retiene el lock. Si no se puede leer el proceso (acceso denegado, lista ilegible), se considera vivo y el lock se conserva (fail-closed).
- **Identidad de ejecutables**: `channel::file_id` da en Windows `(número de serie del volumen, índice de archivo)`. Con eso funcionan `daemon.replace` y la comprobación "servidor = binario instalado" del hook (ADR-GRD-003 § 4).
- **Columna `inode`**: guarda el u64 bit a bit (complemento a dos), así que cabe un índice NTFS con el bit alto puesto. En Unix, un inodo mayor que `i64::MAX` ya no hace fallar la anotación.

**Resultado en la máquina real** (`C:\src\xp15`, `CARGO_BUILD_JOBS=2`):

| Ámbito | `main` (`42b7d1d`) | Esta rama |
|---|---|---|
| `cargo test -p gitraptor-winsys --lib` | 27 pasan, 0 fallan | 29 pasan, 0 fallan |
| `cargo test -p gitraptor-core --lib timemachine::oplog` | 28 pasan, 0 fallan (los de locks no corrían: `cfg(unix)`) | 38 pasan, 0 fallan |
| `cargo test --workspace --no-fail-fast` | — | 1001 pasan, 0 fallan, 3 ignorados |

`cargo clippy --all-targets -- -D warnings` está limpio en la máquina. Los nombres 8.3 están activos en `C:`, así que el test de la ruta corta se ejecutó.

**Tests nuevos en la máquina real**:

- `winsys`: `file_id::tests` (la entrada lee su identidad y borra exactamente lo que fijó; dos grafías de la misma ruta, en mayúsculas o con el nombre 8.3, son el mismo archivo) y `process::tests` (hora de creación de un hijo vivo y `Gone` al terminar).
- `oplog`: el lock de un hijo muerto se libera con el `SystemProbe` real; un PID reutilizado (vivo, con otra hora de inicio) no hereda el lock, y con su hora de inicio exacta el lock espera y se conserva; un lock anotado con otra grafía de sus carpetas se libera igual; una carpeta con nombre de lock nunca se borra; un índice con el bit alto puesto se conserva bit a bit. Los tests de locks que solo corrían en Unix corren ahora también en Windows.

**Sigue pendiente**:

- ~~En Unix, `SystemProbe` sigue usando `kill(0)` y no distingue un PID reutilizado.~~ Resuelto (2026-10-08): en Unix también compara la hora de inicio (macOS exacta; Linux por la parte de menos de un segundo, inmune a los ajustes de reloj). Ver DS-TS-TMC-002, Enmienda 2026-10-08 (Unix).
- Las filas `child-started` anteriores a este cambio no llevan hora de inicio: con ellas, el proceso se considera vivo si el PID existe.
- Que el sistema de archivos no sea NTFS se comprueba al liberar, no al anotar. No se probó en un volumen FAT, exFAT ni ReFS (la máquina solo tiene NTFS).

### TQ-14 en Windows (XP-34, 2026-10-08)

**Decisión de Rene (2026-10-08), opción A**: en Windows, un comando reservado se acepta desde una consola de una sesión interactiva cuando ni el solicitante, ni sus ancestros, ni el host de la consola ni sus ancestros son agentes (DS-TS-GRP-004 § 9, Enmienda 2026-10-08 de ADR-GRP-005). La opción B (Windows Hello) queda en TS-GRP-007.

**Verificado en la máquina real** (Windows 10 Pro 19045, la persona conectada por Escritorio remoto en la sesión 2; la consola física es la sesión 1), con un perfil temporal (`GITRAPTOR_PROFILE_DIR`) y el binario de depuración de la rama:

| Origen | `raptor repo add` | `raptor daemon stop` |
|---|---|---|
| PowerShell en el escritorio de la persona (sesión 2, cadena `raptor → powershell → cmd → explorer`) | **aceptado** ("observing …") | **aceptado** ("stopped") |
| Agente simulado: copia de PowerShell llamada `claude.exe` en el mismo escritorio | rechazado, `agent-ancestry` | rechazado, `agent-ancestry` |
| SSH (sesión 0) | rechazado, `no-controlling-terminal` ("no desde una sesión SSH ni desde un servicio…") | rechazado, igual |

- La auditoría guarda la sesión de Windows de cada proceso y marca el host de la consola: `{"pid":8916,…,"exe":"C:\\Windows\\System32\\conhost.exe","desktop_session":2,"console_host":true}`.
- Tests en la máquina real: `gitraptor-winsys` `console` (2: el kernel da un `conhost.exe` de System32 creado por el hijo con `CREATE_NEW_CONSOLE`, ninguno con `DETACHED_PROCESS`; la sesión 0 no es interactiva) y `gitraptor-core` `windows` (11, entre ellos `peer::windows_tests::reads_this_process_and_its_parent` y `authz::tests::windows_has_a_terminal_proof`).
- **Cómo se simuló a la persona**: desde SSH no se puede teclear en el escritorio, así que la "PowerShell de la persona" se lanzó con una tarea programada `/IT` que abre un `.cmd` con `explorer.exe`. Es justo la clase de intermediario que el riesgo residual acepta (una consola nueva con una cadena limpia), así que prueba el camino de aceptación pero **no** que un agente no pueda imitarlo. Falta que Rene lo confirme tecleando `raptor daemon stop` en su propia consola.
- **Hallado al probar** (corregido en esta rama): `raptor repo add` no llegaba nunca al motor en Windows, porque `canonicalize` da `\\?\C:\…` y el motor lo rechaza como ruta de dispositivo ("invalid parameters"). La CLI quita ahora ese prefijo en las rutas con unidad.
- **Hallado al probar, fuera de alcance**: (a) un cliente elevado (la sesión SSH de un administrador) frente a un daemon sin elevar, arrancado desde el escritorio, se cierra como `peer-unreadable` y la CLI muestra "unexpected end of file" en vez de un rechazo explicado; (b) una cadena que pasa por un proceso de servicio que el usuario no puede abrir (el Programador de tareas, la activación de apps empaquetadas como Windows Terminal desde Inicio) se rompe y se rechaza como `identity-unverified` (fail-closed); (c) `is_session_root` da por raíz cualquier `explorer.exe` de la carpeta de Windows del usuario, también uno recién lanzado por un intermediario (riesgo residual ya aceptado; endurecerlo es comprobar que su padre ya terminó).

## Mantenimiento del índice

- Al añadir o cerrar una marca "Pendiente: etapa de validación multiplataforma" en un artefacto, se actualiza la fila de su XP y el conteo.
- Cuando un XP pasa, la marca se queda en su artefacto de origen hasta que la historia dueña la retire; aquí cambia el estado, con la fecha y el canal.
- **Fuera de alcance de este PR**: un check en `docs-lint` que asocie cada marca a un XP y falle si alguna queda huérfana. Lo propuso el Arquitecto.

## Anexo: marcas por pendiente

Rutas de `docs/requirements/features/` abreviadas (`motor-local/`, `time-machine/`, `cockpit/`, `mcp/` y `guardrails/`).

- **XP-01**: crates/core/src/channel/mod.rs:38; crates/core/src/channel/transport.rs:13; crates/core/src/daemon/mod.rs:417; docs/architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md:229; motor-local/dev-specs/TS-GRP-004-dev-spec.md:115; motor-local/technical-stories/TS-GRP-004-canal-clientes.md:94 (Windows); docs/architecture/decisions/ADR-CKP-003-arquitectura-tui.md:85; docs/architecture/diagrams/seq-ckp-arranque-tui.md:93 (Windows)
- **XP-02**: crates/api/src/clock.rs:23; TS-GRP-004-dev-spec.md:115 (QPC)
- **XP-03**: crates/core/src/channel/peer.rs:153; TS-GRP-004-dev-spec.md:116; TS-GRP-004-canal-clientes.md:94 (Linux)
- **XP-04**: apps/cli/tests/channel_process.rs:12; apps/cli/tests/protected_process.rs:11; apps/cli/tests/repo_state.rs:12; apps/cli/tests/base_branch.rs:8; apps/cli/tests/live_changes.rs:13; apps/mcp/tests/on_demand.rs:3; apps/cli/tests/daemon_process.rs:30; motor-local/dev-specs/US-GRP-001-dev-spec.md:81; motor-local/dev-specs/US-GRP-012-dev-spec.md:74
- **XP-05**: crates/core/src/watch/watchers.rs:11; motor-local/dev-specs/US-GRP-002-dev-spec.md:35, :94
- **XP-06**: crates/core/src/watch/mod.rs:57; crates/core/tests/watch.rs:5; docs/architecture/decisions/ADR-GRP-011-presupuesto-frescura.md:181
- **XP-07**: motor-local/dev-specs/INF-GRP-001-dev-spec.md:113, :131 (Linux), :178 (Linux); docs/architecture/non-functional.md:89
- **XP-08**: INF-GRP-001-dev-spec.md:114, :129, :131 (Windows), :178 (Windows), :182, :183; .github/workflows/repo-intact.yml:45, :52, :81, :88
- **XP-09**: INF-GRP-001-dev-spec.md:184
- **XP-10**: INF-GRP-001-dev-spec.md:141, :178 (workflow)
- **XP-11**: time-machine/dev-specs/TS-TMC-001-almacen-captura-snapshots.md:153 (Linux)
- **XP-12**: crates/core/src/timemachine/protected/backend.rs:271; crates/core/tests/tm_store_capture.rs:5; crates/core/tests/tm_store_safety.rs:6; crates/git/src/tm_write/store/mod.rs:52; crates/git/src/tm_write/mod.rs:57; crates/git/src/tm_write/files.rs:15, :472; TS-TMC-001-almacen-captura-snapshots.md:153 (Windows); time-machine/dev-specs/TS-TMC-003-escritura-aplicador.md:133
- **XP-13**: TS-TMC-003-escritura-aplicador.md:134
- **XP-14**: time-machine/dev-specs/TS-TMC-002-oplog-diario.md:156
- **XP-15** (cerrado 2026-10-08): TS-TMC-002-oplog-diario.md § 11 y Enmienda 2026-10-08
- **XP-16**: time-machine/dev-specs/TS-TMC-004-operacion-protegida-solicitante.md:144
- **XP-17**: TS-TMC-004-operacion-protegida-solicitante.md:102; docs/architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md:110, :310; cockpit/technical-stories/TS-CKP-002-catalogo-ejecutor.md:69; docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md:243
- **XP-18**: ADR-CKP-002-catalogo-operaciones-ejecutor.md:166 (Linux), :358 (Linux); TS-CKP-002-catalogo-ejecutor.md:79 (Linux); cockpit/technical-stories/TS-CKP-003-capa-cockpit-guardrails.md:66 (Linux); docs/architecture/diagrams/seq-ckp-operacion-usuario.md:115 (Linux)
- **XP-19**: crates/git/src/user_ops.rs:35 (editor de rechazo); docs/architecture/decisions/ADR-TMC-005-solicitante-permisos-solape.md:129; ADR-CKP-002-catalogo-operaciones-ejecutor.md:114, :121, :125, :144, :166 (Windows), :285, :358 (Windows); ADR-GRP-005-forma-motor-proceso-segundo-plano.md:268; cockpit/user-stories/US-CKP-020-trabajo-de-otro-actor.md:69, :82; seq-ckp-operacion-usuario.md:115 (Windows); TS-CKP-002-catalogo-ejecutor.md:79 (Windows); TS-CKP-003-capa-cockpit-guardrails.md:66 (Windows); TS-TMC-004-operacion-protegida-solicitante.md:145
- **XP-20**: docs/architecture/decisions/ADR-CKP-001-prediccion-conflictos-merge-en-seco.md:21, :86, :201; cockpit/technical-stories/SPIKE-CKP-001-prediccion-5s.md:28, :92; cockpit/technical-stories/TS-CKP-001-predictor-conflictos.md:76; docs/architecture/diagrams/seq-ckp-prediccion.md:79
- **XP-21**: ADR-CKP-003-arquitectura-tui.md:52; docs/design-system/README.md:198, :217; cockpit/technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md:59; cockpit/dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md:115; cockpit/technical-stories.md:35; cockpit/technical-stories/INF-CKP-001-esqueleto-tui.md:72; cockpit/user-stories/US-CKP-005-terminal-pequena-sin-color.md:82; cockpit/user-stories.md:37
- **XP-22**: cockpit/user-stories/US-CKP-013-abrir-en-editor.md:87; ADR-CKP-003-arquitectura-tui.md:170, :259
- **XP-23**: seq-ckp-arranque-tui.md:93 (Linux)
- **XP-24**: mcp/technical-stories.md:150; mcp/technical-stories/INF-MCP-001-corpus-seguridad-mcp.md:68 (cwd); mcp/user-stories/US-MCP-003-status-del-repo-del-agente.md:55; mcp/user-stories.md:42, :45; mcp/context.md:303, :323, :363, :420, :486; docs/architecture/non-functional.md:136; ADR-MCP-001-servidor-mcp-cliente-daemon.md:60, :269, :294, :313; mcp/user-stories/US-MCP-001-instalar-en-claude-code.md:46; cockpit/user-stories/US-CKP-019-denegada-excepcion-consciente.md:97
- **XP-25**: mcp/user-stories/US-MCP-010-safe-commit-rutas.md:48; cockpit/user-stories/US-CKP-018-crear-worktree.md:90; INF-MCP-001-corpus-seguridad-mcp.md:68 (UNC)
- **XP-26**: guardrails/technical-stories.md:31; guardrails/technical-stories/SPIKE-GRD-002-factor-so-daemon.md:24, :56, :64, :86; guardrails/user-stories/US-GRD-015-cola-de-confirmacion.md:41; guardrails/user-stories/US-GRD-013-comando-edicion-configuracion.md:40; docs/architecture/non-functional-guardrails.md:68; docs/architecture/decisions/ADR-GRD-008-factor-autenticacion-fuera-de-banda.md:19, :60, :162, :236
- **XP-27**: motor-local/technical-stories/INF-GRP-003-pipeline-release.md:82
- **XP-28**: INF-GRP-003-pipeline-release.md:86; docs/architecture/decisions/ADR-GRP-014-pipeline-release-distribucion.md:81
- **XP-29**: motor-local/technical-stories/INF-GRP-004-canales-distribucion.md:83; ADR-GRP-014-pipeline-release-distribucion.md:96
- **XP-30**: crates/testkit/tests/interceptability.rs (sin marca en el código: los tests fallan en rojo a propósito, ver la segunda ronda)
