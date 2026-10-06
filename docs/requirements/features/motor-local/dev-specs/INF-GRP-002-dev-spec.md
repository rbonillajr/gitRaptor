---
id: DS-INF-GRP-002
title: "Dev Spec — Banco de frescura, escala y huella del motor"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-05
updated: 2026-10-05
related:
  stories: [INF-GRP-002, TD-GRP-002, TD-GRP-003, US-GRP-002, US-GRP-007, SPIKE-GRP-002, SPIKE-CKP-001]
  adrs: [ADR-GRP-011, ADR-GRP-010, ADR-GRP-015, ADR-GRP-005, ADR-GRP-006, ADR-GRP-013]
  nfrs: [NFR-04, NFR-05, HUELLA, RES-01, RES-02, RES-04, SEC-06]
tags: [motor-local, banco, ci, rendimiento, latencia, p95, p50, escala, huella, rss, cpu, gate, calibracion, regresion]
---

# Dev Spec — INF-GRP-002: banco de frescura, escala y huella

Plano de ejecución compacto (AADD ligero) de [INF-GRP-002](../technical-stories/INF-GRP-002-banco-frescura-escala.md). Sigue ADR-GRP-011 § 4 con sus enmiendas. Incorpora el gate de huella que Rene añadió el 2026-10-05 por su preocupación por el consumo de recursos.

**Comando único:**

```sh
cargo bench -p gitraptor-cli --bench engine                 # corrida completa, unos 20 min (3 de ellos generan el repo la primera vez)
cargo bench -p gitraptor-cli --bench engine -- --quick      # humo, unos 3 min
```

`ENGINE_BENCH_ROOT=<dir>` conserva y reutiliza el repo de referencia. `--keep` conserva la corrida y `--only footprint,latency,burst,recreation,ahead-behind` limita los experimentos. El informe JSON queda en `<root>/engine-bench-<so>.json`; en CI se resume además en el job summary.

## 1. Decisiones

Todas son **decisiones del orquestador (2026-10-05)**, validadas por el Arquitecto (`nassa-architect:architect`) y por el PO (`nassa-aadd:product-owner`), con los ajustes que pidieron.

| # | Decisión | Validada por |
|---|---|---|
| D1 | **Banco en `apps/cli/benches/engine.rs`** (`harness = false`, build de release). El daemon corre en un **proceso aislado**: el propio binario del banco, relanzado con `--daemon-root`, llama a `daemon::run_process` con la configuración de `raptor daemon` (`DaemonConfig::for_current_user`) y le cambia el perfil por uno temporal y el `HOME` por el del perfil. No se puede usar el `raptor` de release porque **ignora `GITRAPTOR_PROFILE_DIR` (SEC-06)** y correría sobre el perfil real. El detector de agentes se fija en un nombre ficticio, así que **el coste de resolver el actor no se mide como en producción** (ver § 8) | Arquitecto |
| D2 | **Repo de referencia**: perfil `H` de `repogen` (100K commits, 6.000 archivos, determinista con semilla fija), generado una vez y cacheado en CI con `actions/cache`. SPIKE-CKP-001 lo reutiliza con `repogen::profile("H")`. Cada corrida trabaja sobre un clon local nuevo con 10 worktrees | Arquitecto |
| D3 | **Sensibilidad del gate con el evaluador puro** (`crates/testkit/src/freshness.rs`): un retardo sintético en el recomputo que pasa una etapa produce un aviso con la etapa nombrada, y uno que rompe el total produce un fallo. No se inyectan ganchos de test en el motor de release. **Cableado**: el banco falla si a un escenario le falta alguna etapa canónica o tiene valores no finitos | Arquitecto (ajuste D3) |
| D4 | **Holgura del temporizador: constante por SO, sin calibración en ejecución.** Linux: 0,2 ms p95 (constante 0, correcta; debounce efectivo de 75,4 ms). Mac real: 5 ms p95 (constante 10 ms; efectivo de 70 ms, sobrecompensa sin daño). La tolerancia de aviso del debounce es de **+5 ms** sobre los 75 ms efectivos (ruido) | Arquitecto |
| D5 | **Presupuesto calibrado por máquina**: el banco mide la holgura de `recv_timeout` en la misma corrida y suma al presupuesto del debounce y del total el **exceso** sobre la holgura que descuenta el motor. En el Mac real y en Linux el exceso es 0 y el gate queda en 300 ms. **Con más de 100 ms de exceso la máquina es "no apta"**: la latencia se reporta y queda en aviso. Es lo que pasa en el runner `macos-latest`, cuyo temporizador se despierta 92 ms tarde en p50 y 147 ms en p95. La salida prevista es un runner dedicado (ADR-GRP-011 § 4) | Arquitecto (tope de 100 ms) |
| D6 | **Gates de latencia (p95 del motor `t0` → `t_client_recv`)**, bloqueantes: modificar un archivo, `git add`, commit, checkout, crear y borrar un worktree, y la **ráfaga de 1.000 archivos** de ADR-GRP-011 § 4, midiendo por turnos los otros nueve worktrees. La **ráfaga de 10.000 archivos** es un escenario de estrés. Las dos ráfagas tienen en macOS **techos provisionales de no regresión por plataforma y escenario** (máximo medido × 1,25; tabla en § 4), con aviso por encima de 300 ms, hasta cerrar [TD-GRP-002](../technical-stories/TD-GRP-002-motor-bajo-rafaga.md). En Linux, gate de 300 ms sin techo | Arquitecto y PO |
| D7 | **Gates de huella (NFR HUELLA)**, bloqueantes: en reposo, CPU < 1 % y RSS < 150 MiB con 10 worktrees, y como mucho 256 descriptores abiertos. **Pico de RSS en ráfaga**: aviso por encima de 250 MiB (⚠️ ASSUMPTION del PO), **sin techo** hasta que TD-GRP-002 explique su variación. **Retención**: aviso si el RSS no vuelve por debajo de 150 MiB en 60 s (⚠️ ASSUMPTION del PO). La CPU en ráfaga solo se reporta | PO (objetivos), Arquitecto (sin techo de RSS) |
| D8 | **Gate de corrección**: recreación del stream (40 altas y bajas de un worktree con un escritor en otros siete). Cada archivo escrito tiene que verse tras reconciliar. La marca de hueco y la recuperación por la reconciliación periódica las verifican los tests de `crates/core/tests/watch.rs`, porque el daemon de release no permite acortar los 5 min sin un mando de configuración (US-GRP-013) | Arquitecto |
| D9 | **Reproducibilidad**: dos corridas seguidas en la misma máquina coinciden si su p95 total difiere como mucho en max(25 ms, 20 %). Se verifica a mano (§ 9), no en cada PR, por coste | Arquitecto |
| D10 | **Ahead/behind solo se reporta** (sin gate), en una rama a 50K commits de la base, con y sin `commit-graph` | Arquitecto |
| D11 | **CI** (`.github/workflows/engine-bench.yml`): macOS y Linux, en PR y en `main`, con filtro de rutas (`crates/**`, `apps/cli/**`, `Cargo.*` y toolchain; `Cargo.lock` incluido, así que una subida de `notify` pasa el gate). **Windows no se mide**: el cliente del canal es solo Unix (TS-GRP-004). Pendiente: etapa de validación multiplataforma | Arquitecto y PO |
| D12 | **La INF se cierra con el banco**, no con el cumplimiento del motor: que el banco detecte el incumplimiento bajo ráfaga es justo lo que tenía que hacer. TD-GRP-002 queda como **Must antes de cerrar el MVP** | PO |

## 2. Estructura

| Pieza | Qué hace |
|---|---|
| `crates/testkit/src/freshness.rs` | Percentiles (rango más cercano), etapas con las marcas canónicas, presupuestos, techos por plataforma, gates de latencia y huella, y tolerancia de reproducibilidad. Tiene unit tests |
| `crates/testkit/src/repogen.rs` | Perfil `H` (100K commits) |
| `apps/cli/benches/engine.rs` | Repo de referencia, clon con 10 worktrees, daemon aislado, suscriptor sin pantalla, escenarios, medición de huella, informe JSON y job summary, y código de salida |
| `.github/workflows/engine-bench.yml` | Caché del repo, banco y subida del informe como artefacto |

## 3. Medición

- **`t0`**: fin de la escritura en "modificar un archivo" y fin del comando en los escenarios de Git. Se aceptan los eventos publicados mientras el comando aún corre: cuentan con un total de 0 (el cambio ya se veía en `t0`). La detección solo se aísla en "modificar un archivo" y en las ráfagas. Si `t_recv` llega antes de `t0` la detección vale 0: en Linux, inotify notifica la escritura antes de que `write` vuelva.
- **`t_client_recv`**: el hilo lector del suscriptor lo toma con `clock::monotonic_ns()` al recibir la notificación. La muestra es el primer `worktree.state` posterior al inicio de la acción **cuyo estado muestra el cambio esperado**. El archivo tocado alterna entre su contenido commiteado y uno nuevo, para que ningún estado anterior pueda pasar por el de la muestra.
- **Etapas** (§ 2 de ADR-GRP-011): `detection` (`t0` → `t_recv`), `debounce` (`t_recv` → `t_flush`), `compute` y `persist` (reportadas), `recompute` (`t_flush` → `t_persisted`, presupuesto de 150 ms), `publish` (`t_persisted` → `t_client_recv`, 25 ms) y `total` (`t0` → `t_client_recv`, 300 ms).
- **Muestras**: 200 por escenario y 10 de calentamiento descartadas; en crear y borrar un worktree, N/4 (50).
- **Huella**: en Linux, `/proc/<pid>/{status,stat,fd,fdinfo}` (RSS, CPU, descriptores y watches de inotify); en macOS, `ps -o rss=,time=` y `lsof`. La CPU en reposo se mide sobre 30 s, tras 3 s de asentamiento. El pico de RSS se muestrea cada 100 ms.
- **Además se reporta**: el estado final de cada ráfaga, el crecimiento del perfil por estado publicado y el tamaño serializado de los últimos 1.024 eventos (el buffer de reproducción del daemon).

## 4. Gates

| Gate | Nivel | Valor |
|---|---|---|
| p95 total en los seis escenarios y en `burst-1k` (Linux) | **Fallo** | > 300 ms + exceso de holgura |
| p95 total en `burst-1k` y `burst-10k` (macOS) | **Fallo** por encima del techo; **aviso** entre 300 ms y el techo | Mac local: 420 ms y 620 ms. Runner: 615 ms y 585 ms, sobre el total ajustado |
| p95 de una etapa por encima de su presupuesto | Aviso, con la etapa nombrada | Tabla de ADR-GRP-011 § 2 |
| Exceso de holgura > 100 ms | La latencia pasa a aviso y la corrida se declara no apta | — |
| Muestra sin evento en 5 s, o etapa ausente | **Fallo** | 0 |
| CPU en reposo / RSS en reposo / descriptores | **Fallo** | 1 % / 150 MiB / 256 |
| Pico de RSS en ráfaga | Aviso | 250 MiB (⚠️ ASSUMPTION) |
| Vuelta del RSS por debajo de 150 MiB tras la ráfaga | Aviso | 60 s (⚠️ ASSUMPTION) |
| Recreación del stream | **Fallo** | 100 % visible |

## 5. Plan de pruebas (criterio de la INF → prueba)

| Criterio de INF-GRP-002 | Prueba |
|---|---|
| Sensibilidad (retardo que pasa el total → fallo; que solo pasa una etapa → aviso) | `freshness::tests::a_delay_that_breaks_the_engine_total_fails`, `a_stage_over_budget_with_the_total_within_only_warns`, `the_gate_is_on_the_p95` |
| Informe (p50, p95, p99 y máximo por etapa, escenario y SO) | JSON y job summary del banco; `detection_is_only_isolated_when_asked` |
| Reproducibilidad | Verificación manual (§ 9) con `freshness::reproducible` |
| Escala (p95 de los otros nueve durante la ráfaga) | Escenarios `burst-1k` y `burst-10k` |
| Recreación del stream y pérdida silenciosa | Escenario `recreation` del banco; `crates/core/tests/watch.rs` (`the_periodic_reconciliation_recovers_a_lost_change_in_a_gap`, `an_overflow_reconciles_in_a_gap`) |
| Coherencia de nombres | `freshness::tests::marks_use_the_canonical_names` y la comprobación de cableado del banco |
| Huella | Gates de D7; `every_footprint_limit_fails`, `the_burst_peak_only_warns` |
| Calibración de la holgura | `the_excess_timer_slack_widens_only_debounce_and_total`; sonda `timer_slack` del banco |

## 6. Hallazgos que deja el banco

1. **Ráfaga en macOS**: con una ráfaga en un worktree, el p95 de los otros supera los 300 ms (Mac local: 336 ms con 1K y 496 ms con 10K). El recomputo pasa de ~20 ms a cientos. Sin daemon, `git status` no se degrada con la misma ráfaga (33 → 35 ms), ni siquiera con un `F_FULLFSYNC` cada 75 ms (40 ms): **el cuello está dentro del motor**. En Linux las dos ráfagas quedan en 193 ms p95. → TD-GRP-002.
2. **Memoria en ráfaga**: el pico de RSS va de 420 a 830 MiB según la corrida y el SO, y el daemon retiene entre 220 y 345 MiB después. El buffer de reproducción guarda 1.024 eventos `worktree.state` con el estado de los 10 worktrees, y su tamaño es una causa candidata. → TD-GRP-002.
3. **Temporizador del runner de macOS**: se despierta con 92 a 147 ms de retraso. **Riesgo de producto**: un daemon arrancado por launchd con QoS de fondo podría sufrir el mismo coalescing. Pendiente de verificar en dogfooding con el autoarranque (registrado en TD-GRP-002 y en ADR-GRP-010).
4. **Ahead/behind**: con `commit-graph`, `gix` y `git rev-list` empatan (Mac: 30 y 34 ms; Linux: 39 y 17 ms). Sin `commit-graph`, `gix` es igual o más lento (Mac: 297 y 281 ms; Linux: 429 y 191 ms; runner de macOS: 494 y 281 ms). El supuesto de ADR-GRP-010 § 4 queda refutado sin `commit-graph`. Como va en la segunda fase, no afecta a NFR-04.
5. **Crear un worktree** es el escenario sin ráfaga más caro (p95 de 157 a 268 ms): el primer recomputo de un worktree nuevo de 6.000 archivos.

## 7. CI

Un job por SO, `engine bench (<so>)`, de unos 15 a 30 minutos. La protección de rama debería exigir ambos jobs cuando el coordinador lo decida. El informe se sube como artefacto `engine-bench-<SO>`.

## 8. Fuera de alcance y pendientes

- **Cockpit**: `t_render`, el gate de 100 ms y el de extremo a extremo de 500 ms, cuando exista F-001-02 (ADR-GRP-011, Enmienda Cockpit). El escenario de la predicción de conflictos lo mide SPIKE-CKP-001.
- **Time Machine**: el gate del motor todavía no corre con la captura por observación activa, porque no está cableada en el daemon. El p95 < 200 ms del snapshot previo con 1 y 10 worktrees lo cubre el banco `tm_snapshot` (ADR-TMC-006), que no corre en CI. Pendiente.
- **Coste de resolver el actor**: se mide cuando US-GRP-007 detecte sesiones reales. Queda registrado como nota en [US-GRP-007](../user-stories/US-GRP-007-sesiones-claude-code.md).
- **Variante sobre una pseudo-terminal** y medición en una máquina dedicada: no se hacen.
- **Linux**: se mide en CI, pero se verifica de verdad en la etapa multiplataforma. **Windows**: no se mide. Pendiente: etapa de validación multiplataforma.
- **ADR-GRP-015 (consumo de recursos, llegó a `main` durante este trabajo)**: este banco ya aplica el gate de RES-01 (CPU < 1 %), RES-02 (RSS < 150 MiB) y la parte de descriptores de RES-04 (≤ 256), con las cifras objetivo y sin superarlas. **Pendiente**:
  - RES-01: la ventana de 10 min con dos reconciliaciones y la Time Machine activa (hoy, 30 s y sin Time Machine).
  - RES-03: despertares en reposo.
  - RES-04: vigilancias de inotify frente a `max_user_watches` y vuelta a la línea base tras 100 altas y bajas.
  - RES-05: el banco solo reporta el crecimiento del perfil.
  - RES-07: escenario con todos los núcleos saturados, que bloquea TS-GRP-005.
  - El hallazgo del § 6.3 (coalescing de timers) toca a RES-06: un trabajo en clase `utility` o `background` en macOS puede despertarse tarde. TS-GRP-005 debe medirlo con este banco antes de bajar de clase al observador.
- **Agotamiento de watches de inotify**: 3.398 watches en el runner de Linux con 10 worktrees; el modo degradado queda para US-GRP-003 y US-GRP-004.

## 9. Verificación realizada

Ver la tabla de cifras en el PR. Comandos: `cargo test -p gitraptor-testkit --lib freshness`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, el banco completo dos veces en el Mac de Rene (reproducibilidad) y el CI del PR en macOS y Linux.

## Enmienda (2026-10-05): calibración del gate

**Problema.** El job `engine bench (macos-latest)` daba falsos positivos en PR que no tocan el motor:
- En el run 37395980639 (#89), `burst-1k` dio 1.282 ms y `burst-10k` 1.830 ms, frente a techos de 615 y 585 ms. Al relanzarlo, pasó.
- En el run 37402220407 (#91), `burst-10k` dio 608,7 ms frente a 585 ms.

Linux también falló una vez sin motivo (run 37392881094): `worktree-create` dio 366,5 ms frente a 300 ms. Así el gate bloqueaba merges correctos y enseñaba a relanzar sin mirar.

**Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO** con sus ajustes, que se recogen abajo. Enmienda D5, D6 y D7 y añade D13 y D14. Se aplica también como enmienda de [ADR-GRP-011](../../../../architecture/decisions/ADR-GRP-011-presupuesto-frescura.md) § 4 y deja la deuda [TD-GRP-003](../technical-stories/TD-GRP-003-nfr04-maquina-referencia.md).

### Distribución medida

Se usaron 20 corridas del banco por SO en los runners hospedados, con el mismo motor:
- 12 de PR y de `main` (runs 37359042618 a 37401807069).
- 8 con `workflow_dispatch` sobre esta rama sin cambios en el motor (runs 37404943810 a 37404963167).
- Los 3 intentos fallidos que el relanzamiento sobrescribió, sacados de sus logs.

La tabla da el total `t0` → `t_client_recv` en ms, como rango entre corridas.

| Escenario | macOS p50 | macOS p95 | Linux p50 | Linux p95 |
|---|---|---|---|---|
| modify | 160–224 | 242–294 | 91–105 | 92–106 |
| git-add | 156–213 | 239–277 | 90–105 | 91–105 |
| commit | 118–175 | 207–243 | 87–101 | 89–104 |
| checkout | 141–189 | 222–255 | 86–99 | 87–99 |
| worktree-create | 142–220 | 290–372 | 117–185 | 157–**367** |
| worktree-delete | 129–193 | 217–246 | 79–82 | 80–**147** |
| burst-1k | 223–459 | 327–**1.670** | 141–220 | 193–268 |
| burst-10k | 199–446 | 299–**1.831** | 151–240 | 192–**338** |

Hallazgos:

1. **En los escenarios sin ráfaga, el p50 y el p95 son estables** en los dos runners. La excepción es la cola de crear y borrar un worktree en Linux, con una corrida de cada 20 muy por encima.
2. **El p95 de las ráfagas en macOS es ruido del runner**: varía ×6 con el mismo código. El p50 varía ×2.
3. **El exceso de holgura del temporizador en macOS es bimodal**: unos 64 ms o unos 137 ms, según la VM. Con la regla de D5 ("no apta" por encima de 100 ms), **el gate de macOS solo estuvo activo en 1 de cada 3 corridas**, y fue justo en esas donde dio los falsos positivos.
4. **Huella**: la CPU en reposo fue de 0,0 a 0,4 % (límite 1 %), el RSS en reposo de 25 a 38 MiB (límite 150) y los descriptores, 30 y 39 (límite 256). Hay margen de sobra y es estable. El pico de RSS en ráfaga fue de 422 a 718 MiB.

### Decisiones

| # | Decisión | Validada por |
|---|---|---|
| D5 (enmendada) | La regla "máquina no apta" (más de 100 ms de exceso de holgura) aplica **solo al gate del presupuesto** en `--gate reference` | Arquitecto |
| D6 (enmendada) | El **presupuesto NFR-04** (p95 ≤ 300 ms + exceso de holgura, con los techos de ráfaga del Mac de referencia de 420 y 620 ms) **bloquea en la máquina de referencia** (`--gate reference`, por defecto fuera de CI). En los **runners compartidos** (`--gate ci`, por defecto con `GITHUB_ACTIONS`, y explícito en el workflow) **se mide y se reporta** como aviso y en el job summary, sin bloquear. Desaparecen los techos de ráfaga del runner de macOS (615 y 585 ms). El modo se elige de forma explícita, no solo por la variable de entorno (ajuste del Arquitecto) | Arquitecto y PO |
| D7 (enmendada) | La huella en reposo sigue bloqueante en todas partes: es el presupuesto real y tiene margen. Al **pico de RSS en ráfaga** se le añade un **techo de crecimiento de 1.250 MiB** (máximo medido: 830 MiB en el Mac de referencia × 1,5) que **falla sin confirmación**. Es un detector de fugas, no un presupuesto. Cuenta solo la primera ráfaga de cada tamaño, porque un reintento hereda el RSS retenido. Se mantiene el aviso de 250 MiB (TD-GRP-002) | Arquitecto (techo bloqueante), PO |
| D13 | **Gate de regresión en los runners compartidos**, bloqueante. Hay un techo calibrado por runner y escenario sobre la **mediana (p50)** del total, y sobre el **p95** donde el runner lo sostiene: todos los escenarios en Linux y los escenarios sin ráfaga en macOS. En las ráfagas de macOS el p95 solo se reporta. Cada techo es `max(máximo × factor, máximo + 30 ms)`, con factor ×1,25 para el p50 de los escenarios sin ráfaga y ×1,5 para el p50 de las ráfagas y para todos los p95. El margen mínimo de 30 ms evita que los p50 tan estrechos de Linux dejen solo unos milisegundos (ajuste del Arquitecto). Un escenario sin techo calibrado **falla**: no puede pasar en silencio. Si el exceso de holgura del runner supera el de calibración más 50 ms (macOS 190 ms, Linux 50 ms), el runner está **fuera de calibración** y el gate de regresión pasa a aviso con su motivo. La holgura se mide sin daemon, así que eso no oculta regresiones del motor | Arquitecto |
| D14 | **Confirmación, 2 de 3**. Si un escenario supera un techo de regresión, el banco **vuelve a medir el paso entero**, con calentamiento y todas sus muestras, en el mismo daemon. Los pasos son el ciclo de modify, add y commit; el checkout; crear y borrar un worktree; y cada ráfaga. Antes de repetir una ráfaga espera 10 s. **Falla si dos de tres intentos superan el techo** (equivale a la mediana de tres). Un intento que superó el techo sin confirmarse queda como **aviso**. Cada intento figura en el informe (`regression`) y en el job summary, con el número de confirmaciones. Si la confirmación salta en más de 1 de cada 5 corridas, hay que recalibrar (Arquitecto). Se descartó comparar contra una línea base de `main` en el mismo runner: dobla los 15 a 30 minutos del job y el ruido es temporal, no de máquina | Arquitecto |
| D15 | **Recordatorio y trazabilidad del presupuesto** (ajustes del PO). En un PR que toca el motor (`crates/core/src/{watch,daemon,channel}/`, `observe.rs`), el job avisa y pide adjuntar el informe de `--gate reference`. El informe registra las condiciones (`conditions`: commit, si había cambios sin commitear, SO, modelo, núcleos, carga, alimentación y si es CI). Antes de cada release se corre `--gate reference` en la máquina de referencia, como paso de [INF-GRP-003](../technical-stories/INF-GRP-003-pipeline-release.md) → TD-GRP-003 | PO |

### Techos de regresión (`REGRESSION_CEILINGS`, `crates/testkit/src/freshness.rs`)

Máximo medido → techo, en ms. Se recalibran cuando el motor mejore a propósito (por ejemplo, al cerrar TD-GRP-002) o cuando cambie la imagen del runner.

| Escenario | macOS p50 | macOS p95 | Linux p50 | Linux p95 |
|---|---|---|---|---|
| modify | 224,3 → 280 | 293,7 → 441 | 105,1 → 135 | 105,6 → 158 |
| git-add | 212,6 → 266 | 277,0 → 416 | 104,5 → 135 | 105,2 → 158 |
| commit | 174,6 → 218 | 242,8 → 364 | 100,8 → 131 | 103,7 → 156 |
| checkout | 188,9 → 236 | 254,6 → 382 | 98,6 → 129 | 99,1 → 149 |
| worktree-create | 219,7 → 275 | 371,5 → 557 | 185,4 → 232 | 366,5 → 550 |
| worktree-delete | 192,7 → 241 | 246,1 → 369 | 81,5 → 112 | 147,1 → 221 |
| burst-1k | 458,7 → 688 | solo se reporta | 220,0 → 330 | 268,4 → 403 |
| burst-10k | 445,8 → 669 | solo se reporta | 239,8 → 360 | 337,6 → 506 |

En Linux, los techos de p95 de los escenarios sin ráfaga siguen por debajo de 300 ms, así que ahí el gate es más estricto que NFR-04. Las excepciones son crear un worktree y las ráfagas.

**Regresión mínima detectable** (techo de p50 menos el p50 típico, la mitad del rango):
- **Linux**: unos 30 a 40 ms en los escenarios estables, unos 80 ms en crear un worktree y unos 150 ms en las ráfagas.
- **macOS**: unos 70 a 90 ms en los escenarios estables y unos 350 ms en las ráfagas.

Por eso, una regresión de ráfaga en macOS de menos de unos 350 ms solo la ve el gate de la máquina de referencia (TD-GRP-003) o, si no es exclusiva de macOS, el runner de Linux.

### Sensibilidad demostrada

PENDIENTE_SONDAS

### Pruebas

`freshness::tests`: `every_scenario_is_calibrated_on_both_runners`, `a_scenario_without_a_ceiling_fails`, `a_ceiling_keeps_a_minimum_margin`, `the_regression_gate_sees_a_shift_and_a_tail`, `only_the_mac_runner_leaves_the_burst_p95_ungated`, `the_linux_steady_ceilings_are_within_the_budget`, `confirmation_is_two_of_three`, `the_mode_picks_the_platform`, `the_burst_peak_fails_over_its_growth_ceiling`. En local, `--quick --gate ci` con el techo de `modify` y de `checkout` bajado a 31 ms: el banco repitió los dos pasos, confirmó 2 de 2 y falló nombrándolos.

### Estabilidad demostrada

PENDIENTE_ESTABILIDAD

Linux y Windows: el runner de Linux se calibró en CI. **Windows no se mide** (el cliente del canal es solo Unix). **Pendiente: etapa de validación multiplataforma**.
