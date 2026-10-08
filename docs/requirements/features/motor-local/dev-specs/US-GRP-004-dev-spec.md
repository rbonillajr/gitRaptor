---
id: DS-US-GRP-004
title: "Dev Spec — El desarrollador encuentra lo ocurrido aunque no tuviera GitRaptor abierto"
type: dev-spec
status: implemented
feature: motor-local
domain: GRP
created: 2026-10-06
updated: 2026-10-08
related:
  stories: [US-GRP-004, US-GRP-002, US-GRP-007, US-GRP-009]
  enablers: [INF-GRP-001, TS-GRP-003, TS-GRP-004, TS-GRP-005]
  adrs: [ADR-GRP-005, ADR-GRP-010, ADR-GRP-013, ADR-GRP-014, ADR-GRP-015]
  rules: [BR-CONS-005, BR-CONS-001, BR-EDGE-005]
  nfrs: [NFR-01, NFR-06, SEC-05, SEC-06, SEC-10, SEC-14]
tags: [motor-local, continuidad, autoarranque, launchd, systemd, hkcu-run, reinicio, i18n]
---

# Dev Spec — US-GRP-004: observación continua

Plano compacto (AADD ligero) de [US-GRP-004](../user-stories/US-GRP-004-observacion-continua.md). Cierra el eslabón de M1 hacia US-TMC-004: la captura no depende de que haya una superficie abierta y sobrevive a que el motor deje de ejecutarse.

**Qué entrega**:

- El motor ya era un proceso propio que sigue observando sin clientes y persiste el historial, las atribuciones y las sesiones en el perfil antes de publicarlos (ADR-GRP-005 § 4, ADR-GRP-013). Esta historia **lo prueba de punta a punta**, con y sin parada ordenada.
- Añade el **autoarranque real** de ADR-GRP-005 § 3, que solo se activa con `raptor daemon enable` (PQ-1). Su inverso es `raptor daemon disable`.
- Con el autoarranque registrado, **los clientes piden el arranque al gestor de servicios** en lugar de lanzar el motor ellos.
- `scope.snapshot` y `raptor daemon status` dicen si el autoarranque está registrado (antes siempre "desconocido").

## 1. Decisiones

Todas son **Decisión del orquestador (2026-10-06), validada por el Arquitecto**. El Arquitecto la aprobó con ajustes; la columna de la derecha recoge los ajustes ya incorporados. No se consultó al PO porque no hay decisión de producto nueva: el alcance y PQ-1 ya estaban fijados.

| # | Decisión | Ajuste incorporado |
|---|---|---|
| D1 | **`enable` y `disable` se ejecutan en local**, en el proceso de la CLI: no hay método del canal y el MCP no los expone. **No son comandos reservados**, porque ADR-GRP-005 § 6 no los lista. `disable` **nunca para el motor en marcha** (ni `launchctl bootout` ni `systemctl stop`): así un agente no puede usarlo para parar la captura | `daemon.replace` sigue comparando con la ruta desde la que arrancó el daemon, nunca con el artefacto. Riesgo residual escrito en la enmienda de ADR-GRP-005 |
| D2 | **Artefactos** (`crates/core/src/autostart.rs`): **macOS** `~/Library/LaunchAgents/dev.gitraptor.plist` (`RunAtLoad`, `KeepAlive.SuccessfulExit=false`, `ProcessType=Standard`, sin rutas de salida). **Linux** `~/.config/systemd/user/gitraptor.service` (`Restart=on-failure`, `StandardOutput/StandardError=null`, sin `Nice=`, `WantedBy=default.target`) más su enlace en `default.target.wants`, creado por el propio `enable`. **Windows** valor `GitRaptor` en `HKCU\…\Run` = `"<exe>" daemon --autostart`, escrito por FFI en `crates/winsys::registry`. `~` es el HOME de la base de usuarios, no `$HOME` ni `$XDG_CONFIG_HOME` (SEC-10) | Se escribe la **ruta estable** (`current_exe()`, p. ej. el enlace de Homebrew), no la canónica, y SEC-14 se comprueba sobre las dos. Escritura con modo 0644, por temporal y `rename`, sin seguir enlaces. Las **carpetas** se crean si faltan y no se borran (aclaración de la Validación 11 en la enmienda). En Linux, si el systemd del usuario no carga la unidad tras `daemon-reload` (`systemctl --user cat`), se deshace lo escrito y se da un error tipado |
| D3 | **`raptor daemon --autostart`** (oculto): el gestor de servicios lo usa. Si el arranque falla, sale con 0 en lugar de 3 o 1, así `KeepAlive` y `Restart=on-failure` no lo relanzan en bucle (ADR-GRP-015) | Vale para **todo** fallo de arranque (otra instancia, perfil inseguro, E/S), no solo para la carrera de instancia única. Un panic sigue saliendo con un código distinto de 0 y se relanza. Una parada ordenada ya salía con 0 |
| D4 | **`enable` activa ya**: en macOS hace `launchctl bootstrap gui/<uid> <plist>`; si falla (ya cargado, o sesión solo SSH) pregunta con `print`. En Linux hace `daemon-reload` y `start`. Si ya corre un motor bajo demanda, la instancia del gestor sale con 0 (D3). `disable` borra exactamente los artefactos; en Linux hace `daemon-reload` | Documentado: tras `disable` en macOS el job queda cargado hasta el cierre de sesión, y un `enable` con otro contenido no cambia la definición cargada hasta entonces |
| D5 | **Clientes** (`client.rs::launch`): si el artefacto existe y **ejecuta el mismo binario que el cliente** (comparación canónica), se pide el arranque al gestor con `launchctl kickstart gui/<uid>/dev.gitraptor` (si falla, `bootstrap`) o con `systemctl --user start`. Si no, o si el gestor falla, se usa el arranque con entorno limpio de siempre. En Windows siempre entorno limpio. El daemon nunca lanza la herramienta del gestor: para `AutostartView` solo comprueba si el artefacto existe | Herramientas por ruta absoluta fija (`/bin/launchctl`, `/usr/bin/systemctl`) con argv y entorno fijos y tiempo máximo de 5 s. `XDG_RUNTIME_DIR` se deriva como `/run/user/<uid>` y se verifica que sea del usuario y privado. **Nunca `kickstart -k`** (mataría el motor) |
| D6 | **SEC-14**: `enable` se niega si el binario está en el canal npm (`node_modules`) o en las cachés de `npx` (`_npx`), `pnpm dlx` o `bunx`, si está bajo una carpeta temporal (`temp_dir`, `/tmp`, `/var/tmp`, `/var/folders`, `/dev/shm`, `/run/user`, `%SystemRoot%\Temp`) o si otros usuarios pueden modificarlo. Es idempotente: un artefacto idéntico da "ya estaba activado" | ADR-GRP-014 e INF-GRP-004 piden rechazar **todo** el canal npm, no solo `_npx`. Se añade el rechazo de un binario modificable por otros, con la regla de SEC-10 para Git |
| D7 | **Ganchos de test solo en debug** (SEC-06), con fallo cerrado. `GITRAPTOR_AUTOSTART_DIR` cambia la carpeta del artefacto y **exige** `GITRAPTOR_TEST_SERVICE_TOOL`, que sustituye a `launchctl` o `systemctl`. En Windows, `GITRAPTOR_AUTOSTART_REGKEY` cambia la subclave. Con un perfil de prueba (`GITRAPTOR_PROFILE_DIR`) y sin carpeta de prueba **no hay autoarranque**: un test nunca toca el `~/Library/LaunchAgents` real ni carga un job real. El artefacto lleva las variables de prueba (perfil, clasificador, carpeta y herramienta) para que el motor que arranca use el perfil de prueba. Release ni las lee | — |
| D8 | **Pruebas sin tiempos fijos**: se espera a señales explícitas con un tope (la línea `daemon_started` del log del motor, el historial, el estado de un worktree tras una sonda). launchd se simula con un `launchctl` falso que registra su argv y, en `bootstrap` y `kickstart`, ejecuta **literalmente** los `ProgramArguments` y `EnvironmentVariables` del plist que escribió `enable`, fuera de la sesión de terminal | El Arquitecto pidió además tests unitarios de D3 y de la discrepancia de binario de D5: están en `autostart.rs` |

## 2. Estructura

```
crates/winsys   registry.rs, ffi_registry.rs   valores de texto de HKCU (RegGetValueW,
                                               RegSetKeyValueW, RegDeleteKeyValueW); Win32_System_Registry
crates/core     autostart.rs   Autostart: artefactos por SO, enable/disable/start, SEC-14,
                               start_failure_exit (D3), plist/unidad/valor Run y sus parsers
                client.rs      launch(): arranque por el gestor (D5); debug_overrides()
                channel/       ChannelConfig.autostart; scope.snapshot → registered | not-registered | unknown
                daemon/mod.rs  DaemonConfig::for_current_user rellena ChannelConfig.autostart
apps/cli        main.rs (daemon --autostart, daemon enable|disable), autostart.rs, i18n/{en,es}.txt
```

## 3. Plan de pruebas (escenario → test)

Escenarios de punta a punta en `apps/cli/tests/continuous_observation.rs` (macOS). Usan el binario `raptor` real, el repo, el perfil y el home temporales del arnés (INF-GRP-001) con los worktrees `feat-login` y `feat-api`, el desarrollador bajo un pty (`script`), el Claude Code simulado de US-GRP-007 y el launchd simulado de D8.

| Escenario Gherkin | Test |
|---|---|
| La actividad se captura sin ninguna superficie abierta | `activity_is_captured_with_no_surface_open`. `raptor repo add` arranca el motor y termina. Se hacen 2 commits en `feat-login` sin ningún cliente abierto. Después se consulta: los 2 commits están en el historial con su rama, observados dentro de la ventana de su comando de Git (2 s de margen), por el mismo motor; `raptor events` los muestra |
| Lo observado sobrevive a que el motor deje de ejecutarse y vuelva a arrancar | `what_was_observed_survives_the_engine_stopping_and_starting_again` (parada ordenada) y `…_dying_and_starting_again` (`kill -9`). Codex registrado en `feat-login` con un commit suyo, y una sesión de Claude Code en `feat-api` con un commit suyo, ya terminada. Tras el reinicio: el historial es **idéntico** (eventos, secuencias y actores), Codex sigue registrado y presente, y la sesión de Claude Code sigue `ended` con la misma causa y la misma hora |
| La observación se reanuda sola al volver a arrancar el motor | `observation_resumes_by_itself_when_the_engine_starts_again`. `enable` registra el autoarranque y launchd arranca el motor, que observa "demo". El motor muere y launchd vuelve a arrancar el job del plist (`raptor daemon --autostart`, comprobado con `ps`). Un commit posterior en `feat-login` aparece en el historial, observado cuando ocurrió, y el pid no cambia: ningún cliente arrancó otro motor |

**Autoarranque (PQ-1, SEC-14)**, en el mismo archivo:

- `repo_intact_enable_and_disable_change_only_the_plist`: huella del testkit con `Exceptions::autostart` más el perfil del motor; cero diferencias fuera del plist y del perfil, y la carpeta queda vacía tras `disable`.
- `enable_registers_this_binary_with_launchd`: `ProgramArguments` = este binario con `daemon --autostart`, modo 0644, `bootstrap gui/<uid> <plist>`, "ya estaba activado" en en y es.
- `enable_from_a_temporary_copy_is_refused`: una copia en una carpeta temporal se rechaza sin escribir nada ni llamar a `launchctl`.
- `status_shows_the_autostart_and_disable_keeps_the_engine_running`: `raptor daemon status` pasa de "not registered" a "registered" y vuelve; el pid sigue igual tras `disable`.
- `a_client_starts_the_engine_through_launchd`: la última llamada es `kickstart gui/<uid>/dev.gitraptor` y el motor corre con `--autostart`.
- `a_second_engine_started_by_launchd_exits_with_zero`: con `--autostart` sale con 0; sin él, con 3.

`crates/core` (`autostart::tests`, en todos los SO donde aplica): contenido y escapes del plist, de la unidad y del valor `Run` con sus parsers; rechazo de npm, npx, `pnpm dlx`, `bunx` y temporales (solo componentes completos); rechazo de un binario modificable por otros; `enable`/`disable` crean y borran exactamente los artefactos con un gestor falso (macOS y Linux; en Linux sin `/run/user/<uid>` se comprueba que no queda nada escrito); el gestor solo se usa si el artefacto ejecuta el mismo binario (macOS); la regla de salida de D3. `crates/winsys` (`registry::tests`, solo Windows): escribir, leer y borrar un valor en una subclave de prueba propia.

## 4. Riesgos y pendientes

- **Desinstalar sin `disable`**: la fórmula de Homebrew no tiene gancho de desinstalación, así que el plist queda apuntando a un binario que ya no existe. Mitigación pendiente: los caveats de la fórmula (INF-GRP-004) y la detección del artefacto huérfano en `raptor doctor` (US-GRP-018).
- **Aviso en la TUI**: con un motor normal `autostart` ya no vale `unknown`, así que la TUI mostrará el aviso de "sin autoarranque" de ADR-GRP-005 § 3 cuando no esté registrado. Es el comportamiento previsto; se avisa a INF-CKP-001.
- **Requisitos**: Q17 y la verificación de BR-CONS-001 todavía no recogen la excepción PQ-1 (pendiente de las Consecuencias de ADR-GRP-005, del PO).
- **Linux**: el código compila y sus tests unitarios corren en el CI de Ubuntu, pero no se ha probado con un systemd de usuario real. Si el binario es de Linuxbrew, `current_exe()` ya devuelve la ruta resuelta. Pendiente: etapa de validación multiplataforma.
- **Windows**: el valor `Run` se escribe por FFI (revisión de `unsafe` como el resto de `crates/winsys`). `raptor.exe` es de consola, así que lanzado desde `Run` abrirá una ventana visible (necesitará `FreeConsole` o un stub GUI). El canal aún no tiene transporte en Windows. Pendiente: etapa de validación multiplataforma.
- **Auditoría dinámica de `exec`** (INF-GRP-001): `launchctl` y `systemctl` solo los lanza el cliente y nunca el daemon. Si la auditoría profunda de macOS se extiende al cliente, tendrá que admitirlos ahí.

## 5. Validación

- **Arquitecto** (2026-10-06): aprobada con ajustes (tabla del § 1), todos incorporados. Pidió una enmienda corta de ADR-GRP-005 (salida de `--autostart`, `disable` sin parada, comprobación del binario antes de `kickstart`, carpetas frente a la Validación 11), ya aplicada. Ninguna decisión contradice un ADR aceptado; D6 se corrigió para cumplir ADR-GRP-014.

## Estado de la implementación (2026-10-08)

Implementado en: PR #112.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
