---
title: Pendientes de la etapa de validación multiplataforma
status: expanded
generated: 2026-10-05
updated: 2026-10-05
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
| XP-01 | Windows | TS-GRP-004, ADR-GRP-005 § 5, ADR-CKP-003, SEC-08 | Canal local por named pipe: DACL con el SID del usuario, primera instancia, rechazo de clientes remotos, SQOS e identidad del servidor (hoy `TRANSPORT_UNSUPPORTED`) | Máquina Windows | pendiente |
| XP-02 | Windows | TS-GRP-004 | Reloj monotónico con QPC comparable entre procesos | Máquina Windows | pendiente |
| XP-03 | Linux | TS-GRP-004 | Identidad del par con `SO_PEERCRED` y `/proc` (inicio calculado a 100 Hz; falta `SO_PEERPIDFD`) | Contenedor | parcial: el código Linux compila y sus tests unitarios pasan (2026-10-05); no hay tests de proceso del canal en Linux (ver XP-04) |
| XP-04 | Linux | US-GRP-001, US-GRP-002, US-GRP-012, TS-GRP-004, INF-GRP-001 | Llevar a Linux los e2e de proceso que hoy son `cfg(target_os = "macos")` (`script`, `nc -U`, `lsof`) | Contenedor | parcial: `apps/cli/tests/daemon_process.rs` pasa (2026-10-05); `channel_process`, `protected_process`, `repo_state`, `base_branch`, `live_changes`, `mcp/on_demand`, `core/channel` y `core/channel_protected` están sin portar |
| XP-05 | Linux + Windows | US-GRP-002 (D2) | Watcher compartido: `max_user_instances`, agotamiento de `max_user_watches` y handles de Windows al borrar un worktree | VM Linux (Lima) + máquina Windows | parcial: `crates/core/tests/watch.rs` pasa en el contenedor (2026-10-05); los límites del kernel quedan para Lima |
| XP-06 | Linux + Windows | ADR-GRP-011, INF-GRP-002, SPIKE-GRP-002, NFR-04 | Presupuesto de frescura (≤ 300 ms p95), `timer_slack` y tiempos del observador | VM Linux (Lima, como referencia) + máquina Windows | pendiente |
| XP-07 | Linux | INF-GRP-001, ADR-GRP-009 (Validación 7), NFR-01 | Auditoría de `exec` con `strace -f` sin root y suites `repo_intact` en verde | Contenedor + CI `ubuntu-latest` | **pasa** en el contenedor (2026-10-05): 72 tests `repo_intact` con `GITRAPTOR_EXEC_AUDIT=strace`, incluido `kernel_tracer` |
| XP-08 | Windows | INF-GRP-001, TS-GRP-002 | Git en fail-closed (falta el chequeo de ACE), canario `.exe`, ETW o Job Object, daemon fsmonitor y paso no bloqueante del CI | Máquina Windows | pendiente |
| XP-09 | Linux | INF-GRP-001 | Repo de otro uid real (segundo usuario) | Contenedor (con un usuario más) | pendiente |
| XP-10 | Varios | INF-GRP-001 (D11) | `repo-intact.yml` ejecutado en los tres SO | CI | pendiente: se verifica con el CI de este PR |
| XP-11 | Linux | TS-TMC-001 | Almacén de Time Machine: `FICLONE`, respaldo por copia y `fsync` | Contenedor; reflink real en VM (btrfs o xfs) | parcial: los tests `tm_store_*` pasan en overlayfs (2026-10-05), donde `FICLONE` cae al respaldo por copia; el reflink sigue sin probar |
| XP-12 | Windows | TS-TMC-001, TS-TMC-003, ADR-TMC-001 | Almacén y escritura (hoy `Unsupported`): `ReplaceFileW`, `FILE_FLAG_OPEN_REPARSE_POINT`, archivo abierto en un editor | Máquina Windows | pendiente |
| XP-13 | Linux | TS-TMC-003 | Aplicador con `renameat2` (`EXCHANGE` y `NOREPLACE`) | Contenedor | **pasa** (2026-10-05): `tm_apply` en verde con Git 2.38.5, 2.43.0 y 2.56.0 |
| XP-14 | Linux | TS-TMC-002 | Oplog con rustix | Contenedor | **pasa** (2026-10-05) |
| XP-15 | Windows | TS-TMC-002 | Identidad estable de archivo para los locks y `SystemProbe` (hoy da por vivo cualquier proceso) | Máquina Windows | pendiente |
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

## Máquina Windows real

> Receta de la máquina que preparó el coordinador para validar Windows por SSH (`ssh gitraptor-win`). Se documenta a partir de su descripción: este PR no la ejecutó ni la verificó.

1. **OpenSSH Server**: es la capacidad opcional de Windows (`Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0`), con el servicio `sshd` en arranque automático y la regla de firewall del puerto 22. En el Mac, un alias `gitraptor-win` en `~/.ssh/config` con clave pública. Para usuarios administradores, la clave va en `C:\ProgramData\ssh\administrators_authorized_keys`, no en el perfil.
2. **Git for Windows**: instalación estándar en `C:\Program Files\Git`. Es una ubicación conocida del resolvedor (`%ProgramFiles%\Git\cmd\git.exe`, ver `crates/git/src/resolve.rs`).
3. **rustup con la toolchain MSVC** (`x86_64-pc-windows-msvc` o `aarch64-pc-windows-msvc`, según la máquina). Al entrar en el repo, `rust-toolchain.toml` instala la versión fijada.
4. **Visual Studio Build Tools** (workload *Desktop development with C++*: MSVC y Windows SDK) como **tarea programada que corre como SYSTEM**. Ni `winget` ni el instalador de Visual Studio funcionan en una sesión SSH, porque no hay escritorio interactivo y el bootstrapper no avanza. Se registra una tarea con `schtasks /create /ru SYSTEM /sc once ...` (o `Register-ScheduledTask`) que lanza `vs_BuildTools.exe --quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`, se ejecuta con `schtasks /run` y se espera a que termine. Después hay que comprobar que `link.exe` existe y que `cargo build` enlaza.
5. **gh** (GitHub CLI), para clonar ramas y consultar los checks del PR desde la máquina.

**Flujo**: clonar la rama dentro de la máquina; no se comparte el directorio del Mac. Después, `cargo test --workspace` y `cargo test --workspace -- repo_intact` (la auditoría por trampas; ETW sigue pendiente, XP-08), y el resultado se anota en la columna Estado.

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
- **XP-12**: crates/core/tests/tm_store_capture.rs:5; crates/core/tests/tm_store_safety.rs:6; crates/git/src/tm_write/store/mod.rs:52; crates/git/src/tm_write/mod.rs:57; crates/git/src/tm_write/files.rs:15, :472; TS-TMC-001-almacen-captura-snapshots.md:153 (Windows); time-machine/dev-specs/TS-TMC-003-escritura-aplicador.md:133
- **XP-13**: TS-TMC-003-escritura-aplicador.md:134
- **XP-14**: time-machine/dev-specs/TS-TMC-002-oplog-diario.md:156
- **XP-15**: TS-TMC-002-oplog-diario.md:155
- **XP-16**: time-machine/dev-specs/TS-TMC-004-operacion-protegida-solicitante.md:144
- **XP-17**: TS-TMC-004-operacion-protegida-solicitante.md:102; docs/architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md:110, :310; cockpit/technical-stories/TS-CKP-002-catalogo-ejecutor.md:69; docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md:243
- **XP-18**: ADR-CKP-002-catalogo-operaciones-ejecutor.md:166 (Linux), :358 (Linux); TS-CKP-002-catalogo-ejecutor.md:79 (Linux); cockpit/technical-stories/TS-CKP-003-capa-cockpit-guardrails.md:66 (Linux); docs/architecture/diagrams/seq-ckp-operacion-usuario.md:115 (Linux)
- **XP-19**: docs/architecture/decisions/ADR-TMC-005-solicitante-permisos-solape.md:129; ADR-CKP-002-catalogo-operaciones-ejecutor.md:114, :121, :125, :144, :166 (Windows), :285, :358 (Windows); ADR-GRP-005-forma-motor-proceso-segundo-plano.md:268; cockpit/user-stories/US-CKP-020-trabajo-de-otro-actor.md:69, :82; seq-ckp-operacion-usuario.md:115 (Windows); TS-CKP-002-catalogo-ejecutor.md:79 (Windows); TS-CKP-003-capa-cockpit-guardrails.md:66 (Windows); TS-TMC-004-operacion-protegida-solicitante.md:145
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
