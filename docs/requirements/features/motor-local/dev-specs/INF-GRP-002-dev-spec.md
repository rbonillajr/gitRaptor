---
id: DS-INF-GRP-002
title: "Dev Spec — Banco de frescura, escala y huella del motor"
type: dev-spec
status: partially-implemented
feature: motor-local
domain: GRP
created: 2026-10-05
updated: 2026-10-08
related:
  stories: [INF-GRP-002, TD-GRP-002, TD-GRP-003, US-GRP-002, US-GRP-007, SPIKE-GRP-002, SPIKE-CKP-001, TS-GRP-006, US-GRP-020]
  adrs: [ADR-GRP-011, ADR-GRP-010, ADR-GRP-015, ADR-GRP-005, ADR-GRP-006, ADR-GRP-013]
  nfrs: [NFR-04, NFR-05, HUELLA, RES-01, RES-02, RES-04, RES-11, RES-12, SEC-06]
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

**Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO** con sus ajustes, que se recogen abajo. Enmienda D5, D6, D7 y D11, y añade D13 a D16. Se aplica también como enmienda de [ADR-GRP-011](../../../../architecture/decisions/ADR-GRP-011-presupuesto-frescura.md) § 4 y deja la deuda [TD-GRP-003](../technical-stories/TD-GRP-003-nfr04-maquina-referencia.md).

### Distribución medida

Se usaron 20 corridas del banco por SO en los runners hospedados, con el mismo motor:
- 12 de PR y de `main` (runs 37359042618 a 37401807069).
- 8 con `workflow_dispatch` sobre esta rama sin cambios en el motor (runs 37404943810 a 37404963167).
- En Linux, además, las 10 corridas de la primera ronda de aceptación (runs 37418857936 a 37418880656), que destaparon el ruido de la cola del p95. Por eso la aceptación se repitió con corridas nuevas.
- Los 3 intentos fallidos que el relanzamiento sobrescribió, sacados de sus logs.

La tabla da el total `t0` → `t_client_recv` en ms, como rango entre corridas.

| Escenario | macOS p50 | macOS p95 | Linux p50 | Linux p95 |
|---|---|---|---|---|
| modify | 160–224 | 242–294 | 91–105 | 92–**151** |
| git-add | 156–213 | 239–277 | 90–105 | 91–**148** |
| commit | 118–175 | 207–243 | 87–101 | 89–**180** |
| checkout | 141–189 | 222–255 | 86–99 | 87–**135** |
| worktree-create | 142–220 | 290–372 | 90–185 | 157–**367** |
| worktree-delete | 129–193 | 217–246 | 79–82 | 80–**175** |
| burst-1k | 223–459 | 327–**1.670** | 141–220 | 193–268 |
| burst-10k | 199–446 | 299–**1.831** | 151–240 | 192–**338** |

Hallazgos:

1. **En los escenarios sin ráfaga, el p50 es estable** en los dos runners, y el p95 casi siempre. Pero de vez en cuando un runner de Linux va ruidoso durante todo el job, más o menos 2 corridas de cada 30. En esas corridas, todos los p95 de los escenarios sin ráfaga suben de ~100 a 135–180 ms y el p50 no se mueve (run 37418857936, de la primera ronda de aceptación). La confirmación no lo filtra, porque el ruido sigue ahí al repetir. Lo absorben los techos de p95, calibrados con esas corridas.
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
| D16 | **Dónde corre (coste)**. Enmienda D11. Lo pidió el coordinador: el banco de macOS (22 min de media por PR) era la mayor parte del CI por PR y ocupaba los 5 runners de macOS del repo, que también usan los checks obligatorios. Queda así:<br>• **Pull requests**: solo `ubuntu-latest`. Es el runner estable y el más sensible.<br>• **macOS**: en cada push a `main` (con el filtro de rutas), en un **nightly** (`schedule`) y bajo demanda. Una regresión exclusiva de macOS pone `main` en rojo después del merge, no para un PR. Para bisecarla, `workflow_dispatch` admite `-f os=macos -f ref=<sha>`, y el job summary nombra el commit medido.<br>• **Concurrencia**: solo un PR cancela su corrida anterior. Cada push a `main` se mide, y las corridas manuales no se cancelan entre sí.<br>• **Nightly**: se salta un commit que un push ya midió en verde.<br>• **Timeout**: 45 min en Linux y 90 en macOS, porque una regresión confirmada repite sus pasos (la sonda de 150 ms agotó los 60 min de antes en macOS).<br>• **Correr macOS en PR por rutas**: se descartó. `Cargo.lock` cambia con cualquier actualización y se comería el ahorro. En su lugar, un PR que toca `crates/core/src/watch/` recibe un aviso que recomienda lanzarlo en macOS.<br>• **Recalibrar**: si en los 10 primeros nightlies de macOS hay más de 1 falso positivo | Arquitecto (ajustes: concurrencia, matriz, timeout por SO, `ref`, nightly, sin rutas) |
| D15 | **Recordatorio y trazabilidad del presupuesto** (ajustes del PO). En un PR que toca el motor (`crates/core/src/{watch,daemon,channel}/`, `observe.rs`), el job avisa y pide adjuntar el informe de `--gate reference`. El informe registra las condiciones (`conditions`: commit, si había cambios sin commitear, SO, modelo, núcleos, carga, alimentación y si es CI). Antes de cada release se corre `--gate reference` en la máquina de referencia, como paso de [INF-GRP-003](../technical-stories/INF-GRP-003-pipeline-release.md) → TD-GRP-003 | PO |

### Techos de regresión (`REGRESSION_CEILINGS`, `crates/testkit/src/freshness.rs`)

Máximo medido → techo, en ms. Se recalibran cuando el motor mejore a propósito (por ejemplo, al cerrar TD-GRP-002) o cuando cambie la imagen del runner.

| Escenario | macOS p50 | macOS p95 | Linux p50 | Linux p95 |
|---|---|---|---|---|
| modify | 224,3 → 280 | 293,7 → 441 | 105,1 → 135 | 151,0 → 227 |
| git-add | 212,6 → 266 | 277,0 → 416 | 104,5 → 135 | 148,0 → 222 |
| commit | 174,6 → 218 | 242,8 → 364 | 100,8 → 131 | 179,5 → 269 |
| checkout | 188,9 → 236 | 254,6 → 382 | 98,6 → 129 | 135,0 → 203 |
| worktree-create | 219,7 → 275 | 371,5 → 557 | 185,4 → 232 | 366,5 → 550 |
| worktree-delete | 192,7 → 241 | 246,1 → 369 | 81,5 → 112 | 175,0 → 263 |
| burst-1k | 458,7 → 688 | solo se reporta | 220,0 → 330 | 268,4 → 403 |
| burst-10k | 445,8 → 669 | solo se reporta | 239,8 → 360 | 337,6 → 506 |

En Linux, los techos de p95 de los escenarios sin ráfaga siguen por debajo de 300 ms, así que ahí el gate es más estricto que NFR-04. Las excepciones son crear un worktree y las ráfagas.

**Regresión mínima detectable** (techo de p50 menos el p50 típico, la mitad del rango):
- **Linux**: unos 30 a 40 ms en los escenarios estables, unos 80 ms en crear un worktree y unos 150 ms en las ráfagas.
- **macOS**: unos 70 a 90 ms en los escenarios estables y unos 350 ms en las ráfagas.

Por eso, una regresión de ráfaga en macOS de menos de unos 350 ms solo la ve el gate de la máquina de referencia (TD-GRP-003) o, si no es exclusiva de macOS, el runner de Linux.

### Sensibilidad demostrada

Se metió un retardo artificial en el recomputo del daemon (`std::thread::sleep` antes de `t_persisted` en `crates/core/src/daemon/mod.rs`) con commits de prueba, que después se revirtieron. Las cifras son el p50 o el p95 confirmado frente a su techo, en ms.

| Retardo | Runner | Run | Resultado |
|---|---|---|---|
| 150 ms | Linux | [37412533485](https://github.com/rbonillajr/gitRaptor/actions/runs/37412533485) | **Falla** con los 8 escenarios confirmados 2 de 2. modify 253 > 135 (p50), commit 383 > 131, worktree-create 734 > 232, burst-1k 358 > 330 y burst-10k 401 > 360. También falla la recreación del stream (gate de corrección) |
| 150 ms | macOS | [37412533485](https://github.com/rbonillajr/gitRaptor/actions/runs/37412533485) | **Falla**: modify 437 > 280 (p50), git-add 434 > 266, commit 586 > 218, checkout 399 > 236 y worktree-create 2.149 > 275, todos confirmados 2 de 2, más una muestra perdida. El job agotó el timeout de 60 min de entonces mientras terminaba, y de ahí el de 90 min (D16). Las ráfagas se repitieron y no constan como fallo en las anotaciones visibles, que GitHub limita a 10. Cuadra con su regresión mínima detectable, de unos 350 ms |
| 50 ms | Linux | [37412538185](https://github.com/rbonillajr/gitRaptor/actions/runs/37412538185) | **Falla** en 5 escenarios confirmados 2 de 2: modify 153 > 135, git-add 153 > 135, commit 182 > 131, checkout 148 > 129 y worktree-delete 142 > 112. Las ráfagas no la ven, como predice la tabla de sensibilidad. También falla la recreación del stream |
| 50 ms | macOS | — | **No verificado**: se canceló para liberar la cola de macOS a los PR de producto. Por la tabla, un retardo de 50 ms queda por debajo de la regresión mínima detectable en macOS (70 a 90 ms) |

La primera ronda de sondas (runs 37408932800 y 37408956073) destapó un fallo del banco: al confirmar el checkout después de las ráfagas, el archivo tocado seguía modificado en ese worktree y `git switch` abortaba con un panic. Se corrigió restaurándolo antes de repetir el paso, con una prueba local que reproduce el orden.

### Pruebas

`freshness::tests`: `every_scenario_is_calibrated_on_both_runners`, `a_scenario_without_a_ceiling_fails`, `a_ceiling_keeps_a_minimum_margin`, `the_regression_gate_sees_a_shift_and_a_tail`, `only_the_mac_runner_leaves_the_burst_p95_ungated`, `the_linux_steady_ceilings_are_within_the_budget`, `confirmation_is_two_of_three`, `the_mode_picks_the_platform`, `the_burst_peak_fails_over_its_growth_ceiling`. En local, `--quick --gate ci` con el techo de `modify` y de `checkout` bajado a 31 ms: el banco repitió los dos pasos, confirmó 2 de 2 y falló nombrándolos.

### Estabilidad demostrada

El coordinador pidió hacer la validación en Linux, donde es equivalente, y dejar en macOS solo 3 corridas para no quitar runners a los PR de producto. El Arquitecto lo aprobó: macOS ya no bloquea PR, así que un falso positivo allí cuesta un `main` en rojo, no un PR parado.

**Primera ronda en Linux** (`8fa916f`, runs 37418857936 a 37418880656): **9 de 10 en verde**. El run [37418857936](https://github.com/rbonillajr/gitRaptor/actions/runs/37418857936) dio un **falso positivo**: `commit`, p95 de 179,5 ms frente a 155,6 ms, confirmado 2 de 2. En ese job todos los p95 de los escenarios sin ráfaga salieron altos (135–179 ms) y los p50 normales (94–99 ms), es decir, ruido del runner durante todo el job. Se recalibraron los techos de p95 de Linux con esas corridas (D13; siguen por debajo de 300 ms) y la aceptación se repitió con corridas nuevas.

**Segunda ronda en Linux** (`0fe52ae`, main en `71284a3` más este cambio): **10 de 10 en verde, sin ninguna confirmación**, de 12 a 14 min por job. Runs:

[37420534956](https://github.com/rbonillajr/gitRaptor/actions/runs/37420534956), [37420537782](https://github.com/rbonillajr/gitRaptor/actions/runs/37420537782), [37420540503](https://github.com/rbonillajr/gitRaptor/actions/runs/37420540503), [37420542790](https://github.com/rbonillajr/gitRaptor/actions/runs/37420542790), [37420545059](https://github.com/rbonillajr/gitRaptor/actions/runs/37420545059), [37420547574](https://github.com/rbonillajr/gitRaptor/actions/runs/37420547574), [37420550301](https://github.com/rbonillajr/gitRaptor/actions/runs/37420550301), [37420553014](https://github.com/rbonillajr/gitRaptor/actions/runs/37420553014), [37420555567](https://github.com/rbonillajr/gitRaptor/actions/runs/37420555567) y [37420558008](https://github.com/rbonillajr/gitRaptor/actions/runs/37420558008).

**macOS: pendiente.** Las 3 corridas en macOS con el gate final no se lanzaron, porque la cola de macOS tenía PR de producto esperando. Para lanzarlas cuando esté libre: `gh workflow run engine-bench.yml --ref <rama o main> --field os=macos`. Tras el merge, cada push a `main` y el nightly siguen midiendo en macOS. Si en los 10 primeros nightlies hay más de 1 falso positivo, se recalibra (D16). Los techos de macOS se calibraron con 20 corridas de ese runner, pero el gate final **todavía no se ha visto en verde en macOS**.

**Coste** (duración media del job; D16):

| | Por PR, antes | Por PR, ahora |
|---|---|---|
| Linux | 14,5 min (12–18; 19 jobs) | 13,5 min (12–14; 10 jobs) |
| macOS | 22,1 min (17–27; 19 jobs), más la espera en la cola de 5 runners | 0 (corre en el push a `main`, en el nightly y bajo demanda) |

Linux y Windows: el runner de Linux se calibró en CI. **Windows no se mide** (el cliente del canal es solo Unix). **Pendiente: etapa de validación multiplataforma**.

## Enmienda (2026-10-06, US-CKP-001): escenario `tui-modify`

Aplicada desde la [Dev Spec de US-CKP-001](../../cockpit/dev-specs/US-CKP-001-flota-en-vivo.md) (D4). **Decisión del orquestador (2026-10-06), validada por el Arquitecto.** El banco mide la frescura de punta a punta hasta la pantalla (criterio 6 del hito M1).

| Cambio | Detalle |
|---|---|
| **Escenario `tui-modify`** | La `App` real del Cockpit (colas, hilo del canal, `update`, `view`) sobre `TestBackend` 120×40, conectada al daemon aislado del banco por el canal real. Cada muestra escribe el archivo del banco en `wt-3` (`t0`) y avanza la TUI hasta que el frame pintado muestra el nuevo recuento (`t_render`, `metrics.last_render_ns`). Con las mismas condiciones que el resto: perfil H (100K commits) y 10 worktrees. `--only tui-modify` lo corre solo |
| **Presupuesto de punta a punta** | NFR-04: 500 ms p95, más el exceso de holgura del temporizador. Falla en `--gate reference` (con la regla "no apta") y en `--gate ci` solo se reporta |
| **Etapa del Cockpit** | `t_client_recv` → `t_render` ≤ 100 ms p95 (ADR-GRP-011 E2). Falla en los dos modos: es trabajo de CPU sobre `TestBackend` y no depende de la holgura del runner |
| **Regresión en `ci`** | Techo por runner con la confirmación 2 de 3. ⚠️ **Provisional** hasta calibrarlo con tres corridas por runner: los techos de `modify` más 100 ms (`TUI_MODIFY` en `REGRESSION_CEILINGS`) |
| **Dónde corre** | Igual que el banco: Linux en cada PR y macOS en `main`, en el nightly y bajo demanda. El workflow no cambia |

## Enmienda (2026-10-06): escenario `channel_flood`

El test debug `slow_client_and_connection_flood_do_not_starve_the_others` (TS-GRP-004, SEC-08) exigía `p95 < 25 ms`, y con carga fallaba 2 de cada 50 veces (lo midió el PR #129). La regla del proyecto es que los límites de rendimiento van en release o en el banco, nunca en tests debug. Por eso el límite pasa al banco con el mismo presupuesto.

| Cambio | Detalle |
|---|---|
| **Test debug** | Conserva solo lo funcional: el cliente bueno recibe los 3000 eventos, el lento recibe el resync y el límite de conexiones se aplica. Ya no mide tiempos |
| **Bench `channel_flood`** | `cargo bench -p gitraptor-core --bench channel_flood` (release). Usa un daemon en proceso sobre un perfil temporal (NFR-01), con un suscriptor bueno, uno que nunca lee y 100 conexiones extra. Publica 3000 eventos de unos 2 KB en el bus y mide `t_published` → recepción del cliente bueno |
| **Gate** | p95 < 25 ms (presupuesto IPC de ADR-GRP-011 § 4), en `ci` y en la máquina de referencia por igual. Un intento por encima se mide otra vez y el bench falla con 2 de 3 intentos por encima, como el resto del banco |
| **Dónde corre** | Es un paso más del job `engine-bench`: Linux en cada PR y macOS en `main`, en el nightly y bajo demanda. Windows lo salta (el cliente del canal es solo Unix) |

**Decisiones del orquestador (2026-10-06):**
- **Bench propio en `gitraptor-core`, no un escenario de `engine.rs`.** El banco del motor ejecuta el daemon como un proceso aparte, así que no puede publicar en el bus directamente. Medir la inundación con eventos reales del observador mezclaría el canal con el motor. El bench en proceso mide solo el canal, que es lo mismo que medía el test.
- **Sin techos de regresión calibrados.** En release, el p95 queda dos órdenes de magnitud por debajo del presupuesto (macOS: p50 0,08 ms, p95 0,22 ms, máx. 1,22 ms), así que el presupuesto se aplica directamente con la confirmación 2 de 3.

No se consultó al Arquitecto: el presupuesto (ADR-GRP-011) y la regla de dónde van los límites de rendimiento ya estaban decididos. Lo único que se elige aquí es dónde se ubica el bench.

**Verificación:** en macOS (local, release) da p95 0,22 ms en 3000 eventos. En Linux se verá en el job `engine-bench` del PR. **Pendiente: Windows** (no aplica hasta que el canal exista allí).

## Enmienda (2026-10-07): escenario de escala por niveles

**Decisión del orquestador (2026-10-07), validada por el Arquitecto.** Es el gate de RES-11 y RES-12 ([non-functional.md](../../../../architecture/non-functional.md)) para la observación por niveles ([ADR-GRP-010](../../../../architecture/decisions/ADR-GRP-010-observacion-cambios-worktrees.md), Enmienda 2026-10-07, aceptada: **Decisión de Rene (2026-10-07)**). Responde a la preocupación de Rene por tener más de 100 repos clonados. **Queda sin implementar hasta [TS-GRP-006](../technical-stories/TS-GRP-006-observacion-por-niveles.md)**; la parte de la raíz de descubrimiento, hasta US-GRP-020.

| # | Decisión |
|---|---|
| D17 | **Escenario `tiered-scale`** en `apps/cli/benches/engine.rs` (`--only tiered-scale`), con el mismo daemon aislado de D1 sobre un perfil temporal (NFR-01) |
| D18 | **Repos** (`repogen`, deterministas, generados una vez y cacheados en CI como el perfil H, D2). Todos cuelgan de un único directorio, que hace de raíz de descubrimiento. **Pequeños a propósito**: el escenario mide cuánto cuesta observar muchos repos, no repos grandes, porque la escala de un repo grande ya la miden los escenarios de D6.<br>• 5 repos **activos** con el perfil nuevo `S` (1.000 archivos, 200 commits), con 2 worktrees cada uno: 10 worktrees activos, la misma carga que los gates de huella de D7.<br>• 95 repos **dormidos** con el perfil nuevo `XS` (200 archivos, 20 commits), con 1 worktree.<br>• 3 carpetas que son repos no observados (candidatos) y 2 que no lo son.<br>⚠️ **ASSUMPTION**: unos 100 MiB en disco y menos de 1 min para generarlos |
| D19 | **Dormidos sin esperar el umbral**: la configuración del daemon del banco (D1, `DaemonConfig`) fija un umbral de segundos y los intervalos de las redes de seguridad. No es una clave de usuario: `engine.observation.*` no admite valores por debajo de 1 h ni de 30 s (ADR-GRP-010 N7), y el `raptor` de release no lee ningún mando del banco |
| D20 | **Fases**. Cada fase empieza con los 5 repos activos y después 30 s de asentamiento:<br>1. **Solo activos**: los 5 activos, sin dormidos. Es la línea base del mismo daemon.<br>2. **100 observados**: los 95 se añaden y se duermen. Se mide en reposo durante **60 s, con el barrido cada 30 s**. 30 s es el mínimo de la clave, así que el resultado es conservador frente a los 120 s por defecto.<br>3. **Despertar**: 20 muestras de una edición en un dormido distinto (centinela) y 20 de una suscripción de un cliente a un dormido. Se mide el tiempo hasta el estado activo publicado.<br>4. **Retraso por red de seguridad**: con el centinela desactivado en la configuración del banco, 10 commits en dormidos, con el barrido cada 5 s. Se mide el tiempo hasta el evento publicado.<br>5. **Frescura de los activos con el barrido en marcha**: 200 muestras de `modify` en un activo |
| D21 | **Medición**: igual que D7 (CPU con la fórmula del banco, pico de RSS en reposo, descriptores numéricos, watches de inotify en Linux), en las fases 1 y 2. En macOS, además, el número de hilos del daemon (el coste de los streams inactivos, ⚠️ de ADR-GRP-010). Además se reportan el coste del barrido por worktree (fase 2, con su propio temporizador en el informe), el intervalo efectivo de la reconciliación lenta y los contadores del bloque `observation` de `engine.resources` |

**Gates** (se suman a los del § 4):

| Gate | Nivel | Valor |
|---|---|---|
| CPU, RSS y descriptores en reposo con 100 observados (fase 2) | **Fallo** desde el primer día, en `ci` y en `reference` | < 1 %, < 150 MiB y ≤ 256 (los `FOOTPRINT_LIMITS` de D7; RES-11) |
| Watches de inotify con 100 observados (Linux) | **Fallo** | ≤ 50 % de `max_user_watches` (RES-04) |
| Almacén de los dormidos (fase 2) | **Fallo** | 0 archivos del almacén de un dormido abiertos (estructural) |
| Bloque `observation` (fase 2) | **Fallo** | 5 activos, 95 dormidos y 3 candidatos, en los contadores (cableado) |
| Retraso por red de seguridad (fase 4) | **Fallo** | Cada commit publicado en ≤ intervalo del barrido + 2 s (RES-12; corrección) |
| RSS y descriptores marginales (fase 2 − fase 1) | **Aviso** hasta tener línea base | ≤ 16 MiB y ≤ 8 descriptores (RES-11) |
| CPU marginal de los dormidos (fase 2 − fase 1) | **Aviso** hasta tener línea base | ≤ 0,1 % (RES-11). En los runners compartidos, la CPU en reposo varió entre 0,0 y 0,4 % con el mismo código (§ "Distribución medida"), así que una diferencia de 0,1 % no se resuelve allí |
| Despertar p95 (fase 3) | **Aviso** hasta tener línea base | ≤ 2 s (RES-12) |
| Barrido por worktree p95 | **Aviso** hasta tener línea base | ≤ 1 ms (RES-12) |
| Frescura de los activos (fase 5) | **Aviso** hasta tener línea base | p50 y p95 dentro de los techos de `modify` de D13 |

- **Línea base y paso a gate**: con 10 corridas en el runner de Linux, los avisos pasan a techos de regresión por runner con la fórmula de D13 y la confirmación 2 de 3 de D14. Las cifras de RES-11 y RES-12 se quedan como objetivo y nunca se superan sin enmendar ADR-GRP-015.
- **Dónde corre**: igual que el resto del banco (D16). En cada PR, solo Linux (`ubuntu-latest`). macOS, en el push a `main` y en el nightly, nunca lanzado a mano para este escenario. Windows no se mide (el cliente del canal es solo Unix). El workflow no cambia: es un experimento más de `engine.rs`.
- **No en tests de debug**: los límites de rendimiento van en el banco. Las comprobaciones funcionales (0 procesos `git` en el barrido, hueco `dormant`, almacén cerrado) son tests de `crates/core` de TS-GRP-006.
- **Coste**: unos 4 min más por job de Linux. ⚠️ **ASSUMPTION**: se mide en la primera corrida. Si pasa de 6 min, la fase 3 baja a 10 muestras.

Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Enmienda (2026-10-08): banco `idle`, CPU en reposo con sesiones de agente

Implementado en: PR #192.

**Origen.** El registro de dogfooding (PR #188) midió una CPU en reposo de **1,59 %** con 9 sesiones de Claude Code. El objetivo de RES-01 es < 1 %, y es el criterio 5 de M1. La huella de D7 mide el reposo sin ninguna sesión, así que no veía este coste.

**Causa, medida con `sample` en macOS**: el escaneo S1 del detector (ADR-GRP-012, cada 1 s) leía la tabla entera de procesos del usuario. Por cada proceso hacía un `proc_pidinfo` y un `proc_pidpath`, entre 700 y 1.200 procesos por segundo. Era el 97 % de la CPU del daemon en reposo, y el 89 % de ese escaneo eran esas syscalls. En el daemon real, con 9 agentes trabajando, otro 50 % aproximado era la captura de la Time Machine por la actividad de archivos. Esa captura es trabajo, no reposo: ver "Para el registro de dogfooding" más abajo.

| # | Decisión |
|---|---|
| D22 | **Banco `apps/cli/benches/idle.rs`** (`cargo bench -p gitraptor-cli --bench idle`). Usa el daemon aislado de D1 sobre un perfil temporal (NFR-01) y un repo pequeño con `M` worktrees (10 por defecto). Arranca `N` sesiones simuladas (10 por defecto): copias del propio binario del banco con el nombre del agente simulado, cada una en un worktree. Opcionalmente suma `P` procesos extra (`--extra-procs`). Espera a que `sessions.list` muestre las `N` sesiones, deja 5 s de asentamiento y mide la CPU del daemon durante `--idle-secs` (60 s). Informa también del tamaño de la tabla de procesos del usuario, porque el coste del escaneo depende de ella |
| D23 | **Gate del banco**: falla si la CPU en reposo llega al `FOOTPRINT_LIMITS.idle_cpu_pct` (RES-01, < 1 %) o si alguna sesión no se detecta. No entra en el CI, porque el coste depende de la tabla de procesos de la máquina y los runners no tienen sesiones. Es el instrumento local con el que se reproduce el criterio 5 |
| D24 | **Optimización sin tocar intervalos ni presupuestos.** El escaneo S1 sigue cada 1 s y la frescura (< 500 ms p95) no cambia, porque el escaneo no está en el camino de los eventos.<br>1. La ruta del ejecutable solo se lee de los `(pid, inicio)` que aún no se han clasificado (`ProcLister::list_bare` y `ProcLister::exe`), y se vuelve a comprobar el inicio después de leerla, para que un pid reutilizado no preste su ruta.<br>2. En macOS, la tabla sale de una sola llamada `sysctl(KERN_PROC_UID)` (`gitraptor_macsys::process::user_processes`; registro en la Enmienda de ADR-GRP-002). Si no se puede leer o no pasa la comprobación con el propio pid, se vuelve a `proc_pidinfo`.<br>**Decisión del orquestador (2026-10-08), validada por el Arquitecto** |

| D25 | **Churn de compilación en carpetas ignoradas** (`--churn F`, sin gate; lo pidió el coordinador a partir de la lectura real de Rene: 2,15 % de media con workers compilando en 4 worktrees). El banco escribe `F` archivos/s en el `target/` ignorado de los worktrees. Git no ve ningún cambio, pero cada evento llegaba al hilo de su worktree, que lo filtraba uno a uno. Optimización: la tarea del worktree comparte con el enrutador las carpetas que ya sabe ignoradas (`IgnoredPrefixes`, como mucho 32 prefijos), y el enrutador descarta sus eventos antes de enviarlos. Además, compara las raíces por bytes y no por componentes. Un cambio de `.gitignore` o de `info/exclude` vacía las dos cachés, y la lectura completa de esa ventana ve lo que se descartó mientras tanto (NFR-01, test `a_folder_dropped_by_the_router_is_seen_once_no_longer_ignored`). Es el filtro de ADR-GRP-010 § 2 (Enmienda 2026-10-05), adelantado: no cambia lo que se descarta |

**Medición** (Mac de referencia, release, 10 sesiones, 10 worktrees, ventana de 60 s, CPU con `ps`, con una resolución de 10 ms):

| Escenario | Antes | Paso 1 (ruta solo de procesos nuevos) | Paso 2 (+ `sysctl`) |
|---|---|---|---|
| ~690 procesos del usuario (3 corridas) | 0,38 / 0,38 / 0,50 % (mediana 0,38 %) | 0,17 / 0,22 / 0,25 % (mediana 0,22 %) | 0,07 / 0,08 / 0,12 % (mediana 0,08 %) |
| +500 procesos extra (~1.180 procesos) | 0,60 % | 0,30 % | 0,17 % |
| Muestras del hilo `raptor-sessions` en `sample` (20 s) | 68 | 48 | 10 |

| Churn en `target/` ignorado, ~540 archivos/s | 4,65 % | — | 3,37 % (con D25) |
| Churn en `target/` ignorado, ~2.170 archivos/s | 12,3 % | — | 8,9 % (con D25) |

Bajo churn, después de D25, el 98 % de la CPU está en los hilos de FSEvents. Unas dos terceras partes de esa CPU son coste del framework y del kernel por cada *callback* (`mach_vm_deallocate`). notify 8.2, anclado por ADR-GRP-010, crea cada stream con latencia 0 y `kFSEventStreamCreateFlagNoDefer`, así que cada escritura llega en su propio *callback*, y no expone ninguna forma de cambiarlo.

Las *wakeups* no se midieron. El intervalo del escaneo no cambia, así que el número de despertares del hilo es el mismo: lo que baja es el trabajo de cada despertar.

**Para el registro de dogfooding.** Su "reposo" es "ningún evento Git desde la muestra anterior". Con agentes que editan archivos, la Time Machine captura por actividad (quieto 1 s, máximo 5 s), así que una muestra "en reposo" puede incluir trabajo real. Tras este cambio, el criterio 5 se vuelve a medir con 9 o 10 sesiones reales: el banco no basta para cerrarlo.

**Pendiente**:
- **Churn en carpetas ignoradas (macOS)**: el coste que queda depende de la configuración del stream de FSEvents, no de nuestro código. Hay dos vías, y las dos son decisiones de arquitectura sobre ADR-GRP-010, porque exigen sustituir o ampliar notify 8.2:
  - *Rutas de exclusión* (`FSEventStreamSetExclusionPaths`, hasta 8 por stream) para las carpetas ignoradas conocidas. No afecta a la frescura.
  - *Una latencia del stream de algunos ms* que agrupe los eventos. Suma esa latencia a `t_recv`, así que habría que justificarla con el presupuesto de ADR-GRP-011.
  
  No se hace en este cambio.
- Linux: el escaneo lee `/proc/<pid>/stat`, `/proc/stat` y el enlace `exe` por proceso (`SystemProcs::read`). Allí solo se aplica la nueva comprobación de la ruta, sin la mejora de coste: **Pendiente: etapa de validación multiplataforma**.
- Windows: sin detector.

## Estado de la implementación (2026-10-08)

Implementado en: PR #76, #98, #133.

Estado: implementación parcial. Pendiente:
- ADR-GRP-015: ventana de 10 min con la Time Machine activa (RES-01), RES-03, gate de inotify (RES-04), RES-05 y RES-07.
- Runner dedicado para el gate de latencia (TD-GRP-003) y escenario `tiered-scale`.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
