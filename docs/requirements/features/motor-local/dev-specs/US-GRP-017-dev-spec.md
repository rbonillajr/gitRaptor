---
id: DS-US-GRP-017
title: "Dev Spec — El desarrollador ve cuánto consume GitRaptor en su máquina"
type: dev-spec
status: implemented
feature: motor-local
domain: GRP
created: 2026-10-05
updated: 2026-10-08
related:
  stories: [US-GRP-017, US-GRP-001, US-GRP-002, US-GRP-018, US-GRP-019]
  enablers: [TS-GRP-004, TS-GRP-005, INF-GRP-001, INF-GRP-002]
  adrs: [ADR-GRP-015, ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-TMC-001, ADR-TMC-007]
  rules: [BR-GRP-001]
  nfrs: [RES-01, RES-02, RES-04, RES-05, RES-09, RES-10, NFR-01, NFR-10, SEC-08, SEC-12, SEC-MCP-01]
tags: [motor-local, recursos, huella, status, cli, i18n, m1]
---

# Dev Spec — US-GRP-017: `raptor status --resources`

Plano compacto (AADD ligero) de [US-GRP-017](../user-stories/US-GRP-017-consumo-recursos-status.md). Parte de lo que ya está en `main`: el daemon y el canal (TS-GRP-004), el observador (US-GRP-002), la Time Machine con su almacén en `<data>/tm/<repo_id>/` (ADR-TMC-001) y `raptor status` (US-GRP-001). Sigue [ADR-GRP-015](../../../../architecture/decisions/ADR-GRP-015-consumo-recursos.md) § 4 y las NFRs RES-01 a RES-05 y RES-10 de [non-functional.md](../../../../architecture/non-functional.md) § Consumo de recursos.

**Qué entrega**: un método de solo lectura `engine.resources` en el canal y el comando `raptor status --resources [--json]`, que muestra la CPU (media de 10 min y pico), la memoria residente, los descriptores abiertos y las vigilancias del daemon, el disco del perfil y el de la Time Machine por repo, cada valor con su objetivo y si lo cumple. Con el motor parado muestra solo el disco, leído del perfil, sin arrancar el motor.

## 1. Decisiones

Todas son **Decisión del orquestador (2026-10-05), validada por el Arquitecto**. La columna de la derecha recoge los ajustes que pidió y que ya están incorporados.

| # | Decisión | Ajuste incorporado |
|---|---|---|
| D1 | **Mismas definiciones que el gate de INF-GRP-002** (PR #76, aún sin mergear): **CPU** = tiempo de CPU acumulado del proceso (usuario + sistema) entre dos instantes, dividido por el tiempo de reloj, en **% de un núcleo** (lo mismo que `ps -o time=` o `/proc/<pid>/stat` del banco). **RSS** = memoria residente del proceso del daemon (lo que da `ps -o rss=` o `VmRSS`). **Descriptores** = descriptores abiertos del proceso (`lsof -p` o `/proc/<pid>/fd`). Los objetivos son los mismos que `FOOTPRINT_LIMITS` del banco: CPU < 1 %, RSS < 150 MiB, descriptores ≤ 256. No se duplica el medidor del banco: el banco mide desde fuera con `ps`/`lsof`; el daemon se mide a sí mismo en proceso, sin lanzar ningún proceso (SEC-08) | Arquitecto: el RSS y los descriptores de la vista son **instantáneos** (el gate toma el pico en reposo), y en macOS `lsof` cuenta también `cwd`, `txt` y bibliotecas, que `/dev/fd` no cuenta: Pendiente en #76 contar solo descriptores numéricos. **MiB, no MB**: errata en ADR-GRP-015 (2026-10-05). Las cifras viven en un solo sitio del producto, `gitraptor_api::resources::TARGETS`; cuando #76 esté en `main`, `FOOTPRINT_LIMITS` puede leerlas de ahí (no se tocan los ficheros de #76) |
| D2 | **Medición en proceso por SO**: CPU con `getrusage(RUSAGE_SELF)` (`nix`, envoltorio seguro) en Unix. RSS: macOS con `proc_pidinfo(PROC_PIDTASKINFO)` (`libproc`, ya es dependencia), Linux con `/proc/self/statm`. Descriptores: entradas de `/dev/fd` (macOS) o `/proc/self/fd` (Linux), menos el propio descriptor del listado. Windows: `GetProcessTimes`, `K32GetProcessMemoryInfo` (`WorkingSetSize`) y `GetProcessHandleCount` en `gitraptor-winsys`, el único crate con `unsafe`. Un valor que el SO no da es `null` ("no disponible"), nunca un cero | Arquitecto: en Windows los handles **no tienen objetivo** (`targets.open_fds = null`, se muestran sin evaluar) hasta tener línea base. El Arquitecto creía que `nix` no tenía el feature `resource`; sí lo tiene (0.31.3) y se usa. Pendiente: etapa de validación multiplataforma para Linux y Windows |
| D3 | **Ventana de CPU**: un hilo `raptor-resources` del daemon toma una muestra cada 10 s y guarda 60 (10 min, RES-01). La **media** es (CPU ahora − CPU de la muestra más antigua) / (reloj ahora − reloj de esa muestra), con una muestra en vivo en cada petición: así hay valor aunque el daemon lleve menos de 10 s. El **pico** es el máximo de los intervalos de 10 s de la ventana, incluido el intervalo en curso. La respuesta lleva `window_s`, los segundos que de verdad cubre la media (menos de 600 si el daemon es más joven). Es la misma media que el gate; el gate mide 30 s y esto 10 min, y lo dice | Arquitecto: la muestra de CPU en vivo se toma **antes** de recorrer el disco, y el disco se reutiliza 60 s, para que el recorrido no se cuele en la CPU que se reporta. El hilo duerme con `recv_timeout` sobre un canal de parada: un despertar cada 10 s (dentro de RES-03) y se para con el daemon sin esperar. La ventana es pura y se prueba con muestras sintéticas, sin relojes reales |
| D4 | **Vigilancias**: `roots` = raíces vigiladas por el observador (en macOS, un stream de FSEvents por raíz; en Linux y Windows, las raíces del watcher compartido). En Linux además `inotify` = vigilancias reales de inotify del proceso (líneas `inotify wd:` de `/proc/self/fdinfo`) y `max_user_watches`; su objetivo es ≤ 50 % del máximo (RES-04). En macOS y Windows `inotify` es `null` | — |
| D5 | **Disco** (`crates/core::resources::disk`, compartido por el daemon y por la CLI con el motor parado): tamaño **en disco** (bloques asignados en Unix; longitud en Windows) de los ficheros del perfil. `profile_bytes` = carpetas `data`, `config`, `state` y `run` **sin** `<data>/tm` (RES-05, objetivo ≤ 250 MiB). `time_machine` = una entrada por carpeta `<data>/tm/<repo_id>` (almacén y oplog), también de repos retirados. Sin seguir enlaces simbólicos. Tope de 2 s y 200 000 entradas: si se alcanza, `complete: false` y la CLI lo dice ("al menos") | Arquitecto: el total de la Time Machine se muestra contra los 10 GiB por defecto de RES-09 solo como **referencia**, sin cumple/no cumple (`within: null`), porque el tope no se aplica hasta US-TMC-022. Las vigilancias (`roots`) y cada repo de la Time Machine no tienen NFR: se muestran "sin objetivo". El escenario 1 ("cada valor muestra su objetivo y si lo cumple") aplica a los valores con NFR (CPU, RSS, descriptores, inotify y disco del perfil) |
| D6 | **Contrato** `engine.resources` (nuevo módulo `crates/api::resources`): `NoParams` → `ResourcesResult { process {cpu {mean_pct?, peak_pct?, window_s}, rss_bytes?, open_fds?}, watches {roots, inotify?}, disk {profile_bytes, time_machine [{repo_id, bytes}], complete}, pools?, power_saving?, targets }`. Solo números, booleanos y enums: **ninguna cadena de presentación** (NFR-10). `pools` (clase activa por pool, TS-GRP-005) y `power_saving` (US-GRP-019) ya tienen tipo y llegan `null`: la CLI muestra "no disponible". **No reservado** (es de solo lectura, como `engine.snapshot`) y **fuera del perfil `mcp`** (SEC-MCP-01) | Aditivo: `API_VERSION` 5.0.0 → 5.1.0 y `PROTOCOL_VERSION` sigue en 5 (como `events.history` en 3.1; al rebasar sobre US-GRP-007, que subió a 5, se tomó la siguiente menor libre). La CLI comprueba en `hello.methods` que el daemon lo ofrece y, si no, pide reiniciarlo |
| D7 | **Objetivos**: en el contrato (`targets`), con los valores de `gitraptor_api::resources::TARGETS`. La CLI evalúa "cumple / fuera de objetivo" con los objetivos que manda el daemon y, con el motor parado, con `TARGETS`. Un valor `null` no se evalúa. **El código de salida no depende del cumplimiento** (escenario 2): 0 si se pudo medir | Hook de pruebas solo en builds de debug, como `GITRAPTOR_AGENT_EXECUTABLES`: `GITRAPTOR_RESOURCE_TARGETS=rss_bytes=1` baja un objetivo para el escenario 2 con un daemon real. Arquitecto: parseo estricto (cualquier error descarta todo el override) y **solo puede bajar** un objetivo; lo lee el **daemon** (el cliente lo pasa en el entorno limpio del daemon a demanda, como los otros hooks). Los builds de release no lo leen |
| D8 | **CLI**: `raptor status --resources [--json]`. **Con el motor parado no lo arranca** (escenario 4): usa `Client::connect` y, con `NotRunning`, mide el disco con D5 y lee los nombres de los repos del índice **en solo lectura**, por una ruta propia (no `sqlite::open`, que crea, migra y fija WAL); si el índice no se puede leer, muestra el `repo_id`. En texto, una línea por valor: `CPU: 0.12 % average over 10 min (peak 0.40 %) — target < 1.00 %: ok`. Fuera de objetivo: `memory (RSS): 210.3 MiB — OVER TARGET (target < 150.0 MiB)`. Textos en/es en los catálogos de la CLI; las rutas, saneadas (SEC-12). En JSON, los mismos valores en unidades fijas (porcentaje, bytes y recuentos), con `target` y `within` por valor, `running` y `repo_id`/`path` por repo | Arquitecto: una conexión `READ_ONLY` a una base WAL crea igualmente `-wal` y `-shm`, y el test lo confirmó. Por eso el índice se abre `immutable=1` **solo si no existe `-wal`** (todo está en el fichero principal y no hay nada que recuperar); con `-wal` (motor en marcha o caído) no se lee y se muestra el `repo_id`. Nunca se crea, migra ni recupera. Test: ningún fichero del perfil aparece ni cambia |
| D9 | **MCP**: `raptor-mcp` no expone nada de esto: el método no está en el perfil `mcp` (no aparece en `hello.methods` y llamarlo da "method not found") y el servidor MCP no publica ninguna herramienta de recursos | — |

## 2. Ficheros

| Fichero | Cambio |
|---|---|
| `crates/api/src/resources.rs` | Tipos del contrato, `TARGETS`, enums de pool, clase y ahorro de energía (preparados para TS-GRP-005 y US-GRP-019) |
| `crates/api/src/methods.rs`, `lib.rs` | `ENGINE_RESOURCES`, no reservado, fuera del MCP; `API_VERSION` 5.1.0 |
| `crates/core/src/resources/{mod,meter,window,disk}.rs` | Medidor en proceso por SO, ventana de CPU (pura), disco del perfil y monitor (hilo de muestreo) |
| `crates/winsys/src/process.rs` (+ FFI) | Tiempos, working set y handles del proceso en Windows |
| `crates/core/src/watch/{mod,watchers}.rs` | Recuento de raíces vigiladas |
| `crates/core/src/daemon/mod.rs`, `channel/{server,conn}.rs` | Arranque y parada del monitor; despacho de `engine.resources` |
| `crates/core/src/profile/index.rs` | Lectura de solo lectura de los repos del índice (motor parado); test en `crates/core/tests/profile_read_only.rs` |
| `apps/cli/src/{main,resources}.rs`, `apps/cli/i18n/{en,es}.txt` | Comando y presentación |
| `docs/architecture/design/api-contract-ipc.md` | Método `engine.resources` y versión 5.1.0 |

## 3. Tests (uno por escenario o criterio)

Con el `raptor` real como daemon y como cliente, sobre la máquina temporal del arnés INF-GRP-001 (repo temporal y perfil temporal; nunca este repo ni el perfil real, NFR-01) y sin tiempos fijos: se espera a señales explícitas (el daemon contesta, el estado lo dice).

| Escenario / criterio | Test (`apps/cli/tests/resources.rs`, salvo indicación) |
|---|---|
| 1. El desarrollador ve el consumo del motor | `the_developer_sees_the_engine_consumption`: "demo" con 3 worktrees; texto con CPU media y pico, memoria, descriptores, vigilancias, disco del perfil y de la Time Machine de "demo", cada uno con su objetivo y ✓. Además, la huella del testkit (`check`) prueba que medir no toca el repo |
| 2. Un valor fuera de objetivo se señala | `a_value_over_target_is_flagged`: daemon con `GITRAPTOR_RESOURCE_TARGETS=rss_bytes=1`; la memoria sale "fuera de objetivo" con el objetivo al lado y el código de salida es el mismo (0) que con todo dentro |
| 3. Salida para scripts | `json_output_has_fixed_units_and_no_presentation_text`: números en %, bytes y recuentos; ninguna cadena salvo enums, `repo_id` y `path`; ningún contenido del repo |
| 4. Sin el motor no se arranca solo para medir | `without_the_engine_it_does_not_start_it`: motor parado; ve "no está en marcha" y el disco del perfil y de la Time Machine; ningún daemon después |
| 5. Un agente no ve el consumo por MCP | `an_mcp_connection_does_not_see_resources` (canal: `hello.methods` sin `engine.resources` y "method not found") y `apps/mcp/tests/handshake.rs` (`tools/list` vacío de recursos) |
| Ventana de CPU (D3) | unit tests de `resources::window` con muestras sintéticas: media, pico, ventana parcial y recorte a 10 min |
| Disco (D5) | unit tests de `resources::disk`: sin la Time Machine, por repo, sin seguir enlaces, tope que deja `complete: false` |
| Medidor (D2) | unit test por SO: el proceso de test tiene RSS > 0, CPU que crece y al menos 3 descriptores |
| Contrato (D6) | unit test de `crates/api`: el método no es reservado ni MCP; `ResourcesResult` rechaza campos desconocidos y no tiene campos de texto |
| i18n (NFR-10) | `catalogs_match` existente y un test de que cada clave nueva está en en/es |

## 4. Pendientes

- **Linux y Windows**: el medidor y el disco están escritos para los tres SO; los tests de punta a punta son de macOS. **Pendiente: etapa de validación multiplataforma**.
- **INF-GRP-002 (#76)**: cuando esté en `main`, que `FOOTPRINT_LIMITS` lea sus cifras de `gitraptor_api::resources::TARGETS` para que el gate y la vista no puedan divergir. No se hace aquí para no tocar los ficheros de #76.
- **TS-GRP-005** rellena `pools` y **US-GRP-019** rellena `power_saving`; los tipos ya están en el contrato.
- **US-GRP-018** (`raptor doctor`) reutiliza `resources::disk` y `TARGETS` con el motor parado.
- **US-TMC-022**: si la Time Machine de Rene pasa de 2 GiB durante el dogfooding, sube a M1 (backlog § Hito M1).

## Enmienda (2026-10-08): niveles de observación en la vista

Cubre el escenario de la Enmienda (2026-10-07) de la historia ("repos activos y dormidos y lo que cuesta cada nivel"). TS-GRP-006 ya publica el bloque `observation` de `engine.resources` (ADR-GRP-010 N8, capacidad `observation.tiers`) y `Client::connect` acepta todas las capacidades, así que la CLI ya lo recibe: **falta solo la presentación**. Sin cambio de contrato ni del perfil `mcp`.

| # | Decisión |
|---|---|
| D10 | **Decisión del orquestador (2026-10-08), validada por el PO.** Por nivel se muestran los repos, worktrees y vigilancias; para los dormidos, el coste de sus redes de seguridad (intervalo del barrido, intervalo efectivo de la reconciliación y CPU de las redes, si el daemon la da); y los worktrees sondeados sin vigilancia (degradados). Los descriptores **no** se reparten por nivel: en macOS un stream de FSEvents por raíz no consume un descriptor por vigilancia y en Linux las vigilancias de inotify comparten uno; se muestran una vez, los del proceso. Ajuste del PO: el criterio de la historia se corrige para decir eso |
| D11 | **Texto**: una línea `repos observados: activos N · dormidos N · despertando N` y, debajo, una por nivel (worktrees y vigilancias; el recuento de repos no se repite y la forma `etiqueta: N` evita los plurales). Sin bloque `observation` (daemon anterior o el observador aún no arrancó): `repos observados por nivel: no disponible`. Con el motor parado no se muestra nada de niveles. **JSON**: `engine.observation` es el bloque del contrato tal cual (recuentos, segundos y %), `null` si el daemon no lo da |
| D12 | **Test de punta a punta** (`apps/cli/tests/resources_tiers.rs`): el `raptor` real como cliente y el daemon **en proceso** con un `TierConfig` corto (comprobación cada 50 ms, umbral de 1 h), porque el binario real tiene como mínimo un umbral de 1 h y un barrido de 30 s. El repo dormido se siembra con actividad de hace 2 h. **No se añade ningún hook** para acortar los umbrales del binario: los mínimos del producto solo se saltan dentro del test (nota del coordinador) |
| D13 | **Repos descubiertos** (US-GRP-020, PR #170): no se observan hasta aceptarlos, así que no consumen nada. Con el motor en marcha, si el daemon ofrece `discovery.candidates` (de solo lectura, fuera del MCP), la CLI cuenta los candidatos (si la lectura falla, la línea no se muestra y el resto de la vista sigue) y muestra `repos descubiertos, sin observar: N (0 recursos hasta aceptarlos)`; en JSON, `engine.discovered_repos` (recuento, `null` si el daemon no lo ofrece). Sin cambio de contrato: el bloque `observation` no lleva descubrimiento. Con el motor parado no se muestra |

**Tests**: `text_in_english_shows_the_tiers`, `text_in_spanish_shows_the_tiers` y `json_has_the_tier_counts` (1 repo activo y 1 dormido, perfil temporal); `resources::tests::without_tiers_they_are_not_available` (unit).

**Tests de D13**: `resources::tests::discovered_repos_are_shown_at_zero_cost` (unit) y la aserción de `discovered_repos` en `json_has_the_tier_counts`.

## Estado de la implementación (2026-10-08)

Implementado en: PR #93, #173.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- `pools` (TS-GRP-005) y `power_saving` (US-GRP-019) se rellenan en sus historias.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
