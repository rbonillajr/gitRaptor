---
id: DS-TS-GRP-006
title: "Dev Spec — Observación por niveles: activo, dormido con centinela y despertar"
type: dev-spec
status: in-progress
feature: motor-local
domain: GRP
created: 2026-10-07
updated: 2026-10-07
related:
  stories: [TS-GRP-006]
  adrs: [ADR-GRP-010, ADR-GRP-011, ADR-GRP-013, ADR-GRP-015, ADR-GRP-007, ADR-GRP-009, ADR-GRP-016, ADR-TMC-004]
  nfrs: [NFR-01, NFR-04, NFR-05, RES-03, RES-11, RES-12]
tags: [motor-local, recursos, escala, niveles, dormido, centinela, despertar, res-11, res-12, nfr-01]
---

# Dev Spec — TS-GRP-006: Observación por niveles

Plano de ejecución compacto (AADD ligero) de [TS-GRP-006](../technical-stories/TS-GRP-006-observacion-por-niveles.md). El diseño está fijado y aceptado por Rene (2026-10-07) en la [Enmienda (2026-10-07) de ADR-GRP-010](../../../../architecture/decisions/ADR-GRP-010-observacion-cambios-worktrees.md#enmienda-2026-10-07-observación-por-niveles) (N1 a N8), con las de ADR-GRP-011, ADR-GRP-013 y ADR-GRP-015. Esta spec no rediseña: traduce N1 a N5, N7 y N8 al código de `crates/core::watch` y `crates/core::daemon`. **NFR-01 es innegociable**: dormir no puede reducir la protección (Q49).

## 1. Código que se toca (convenciones observadas)

| Pieza | Dónde | Qué hace hoy |
|---|---|---|
| Router y tareas | `crates/core/src/watch/mod.rs` (`Shared::route`, `Observer::watch_repo`, `Observer::forget_repo`) | Un hilo por worktree y uno por repo; el router reparte por el prefijo vigilado más largo |
| Tarea de worktree | `watch/worktree.rs` (`run`, `Task::flush`, `Task::periodic`, `IgnoreCache`) | Debounce fijo, reconciliación periódica de 5 min, filtro de ignorados antes del debounce |
| Tarea de repo | `watch/repo.rs` (`RefsView::read`, `classify`, `fingerprint`, `Task::poll`) | Sondeo de respaldo de 30 s; `classify` ya reconstruye los eventos de Git desde el reflog de cada rama, uno por entrada y en orden |
| Bucle del daemon | `daemon/mod.rs` (`Daemon::run`, `Control`), `daemon/repos.rs` (`observe`, `observed`) | Único escritor; persiste antes de publicar; descarta los lotes de un repo sin almacén abierto |
| Recursos | `crates/core/src/resources/`, `crates/api/src/resources.rs` | `engine.resources` (US-GRP-017) |

Convenciones que se siguen: mensajes `WtMsg`/`RepoMsg` por canal `mpsc`, un hilo con nombre `raptor-*` por tarea, ganchos `ObserverHooks` para avisar al daemon, `GapCause` como `text_enum!` en `profile/store.rs`, tests de integración en `crates/core/tests/observe.rs` con repos temporales y esperas por señal (sin `sleep` fijos).

## 2. Decisiones

Todas son **Decisión del orquestador (2026-10-07)** dentro del diseño ya validado por el Arquitecto y aceptado por Rene en la Enmienda de ADR-GRP-010. Ninguna cambia el diseño; fijan cómo se implementa.

| # | Decisión | Origen |
|---|---|---|
| D1 | **El nivel vive en el observador**, por repo: `Tier::{Active, Waking, Dormant}` en `RepoEntry`. El daemon lo pide (`Observer::sleep_repo`) y el observador lo aplica | N1 |
| D2 | **Dormir** = vaciar las ventanas (cada tarea hace su `flush` final antes de parar), parar los hilos de las tareas (se suelta su estado residente: `last`, cachés, `IgnoreCache`), conservar las vigilancias del SO y guardar en `RepoEntry` la `RefsView` del último lote como marca "procesado hasta" y la huella de metadatos (D5). No se retira ni se recrea ninguna vigilancia | N1 pasos 1 a 5 |
| D3 | **Centinela**: el router entrega los caminos de un repo dormido a un único hilo `raptor-sentinel`, que aplica el mismo filtro de ignorados (`IgnoreCache`, sacado de `worktree.rs` a un módulo compartido) y descarta `objects/`. El primer camino que pasa marca el repo `Waking` (compare-and-set) y llama al gancho `repo_wake(repo_id, WakeCause::Sentinel)`. Una ráfaga produce un único despertar porque los siguientes eventos ven `Waking`. El hilo del router (callback de `notify`) no lee Git | N2 |
| D4 | **Despertar sin hueco**: `Observer::wake_repo(repo_id, read)` registra las tareas con la `RefsView` guardada como vista anterior de la tarea del repo, así `classify` publica los commits hechos mientras dormía, uno por entrada del reflog y en orden (reutiliza § 6, la ASSUMPTION de N4 se resuelve: ya lo hace). Cada tarea de worktree relee al arrancar, con su vigilancia ya activa, como hace hoy `watch_repo`: lo escrito durante el despertar llega en esa lectura. La tarea de repo hace un `flush` inicial | N4 |
| D5 | **Huella de metadatos** (`watch/sweep.rs`): por worktree, el contenido de `HEAD` (≤ 4 KiB) y tamaño + mtime de `logs/HEAD`, `index` y los marcadores; por repo, `packed-refs`, `FETCH_HEAD`, la lista de `.git/worktrees/` y el mtime de los directorios de `refs/heads` (recursivo, solo directorios). Solo `symlink_metadata` y una lectura acotada; nunca `RepoReader` ni un proceso `git` | N3 |
| D6 | **Barrido**: un único hilo `raptor-dormant-sweep` del observador, con un despertar por ciclo (`dormantPollSeconds`). Compara la huella de cada dormido con la guardada; si difiere, despierta el repo con `WakeCause::SafetyNet` y el lote del despertar lleva un hueco `dormant` (D8). En el mismo ciclo revisa el umbral de los activos (D9) | N1, N3 |
| D7 | **Reconciliación lenta**: en el mismo hilo, como mucho un repo dormido por ciclo, cuando su última reconciliación supera `dormantReconcileMinutes`. Lee cada worktree con `observe::read_worktree` y compara el `fingerprint` (huella de cambios sin commitear) con el del último estado conocido. **Presupuesto**: mide la CPU de cada reconciliación y alarga el intervalo efectivo para que la media no pase del 0,1 % de un núcleo; el intervalo efectivo se expone (D11) | N3 |
| D8 | **Hueco `dormant`**: nueva variante `GapCause::Dormant => "dormant"`. Lo abre solo un despertar de una red de seguridad que encontró diferencias, desde la red anterior hasta ahora, con los eventos "sin atribuir". El contador `dormant_diffs` va al log como `periodic_diffs` | N5; ADR-GRP-013 (Enmienda) |
| D9 | **Umbral y condiciones para dormir**: el daemon decide (sabe de sesiones, suscripciones y almacenes). Un repo duerme si su última actividad supera `dormantAfterHours`, no tiene ninguna sesión presente, ningún cliente suscrito a ese repo y ningún worktree degradado. La última actividad es la hora del último lote publicado de ese repo, persistida en el almacén con el "observado hasta" que ya existe | N1 |
| D10 | **Disparadores del despertar en el daemon**: el centinela y el barrido (gancho del observador → `Control::Wake`), una sesión nueva en el repo (`sessions_changed`), una petición sobre el repo (`EventHistory`, `RawEvents`, `Guard`, `GuardLog`) y volver a añadirlo (`add_repo`). Despertar abre el almacén, reconcilia (`observe::reconcile`), persiste, publica `waking` y luego `active` cuando las tareas arrancan. Las peticiones esperan al almacén abierto, no a la reconciliación | N4 |
| D11 | **Contrato aditivo** (capacidad `observation.tiers`): `tier` en `RepoView` (`active` · `waking` · `dormant`, `#[serde(default)]`, con `checked_at` opcional) y el bloque opcional `observation` de `engine.resources`. Solo números y enums (NFR-10) | N8 |
| D12 | **Configuración**: `engine.observation.{dormantAfterHours, dormantPollSeconds, dormantReconcileMinutes}` en `crates/policy` con sus niveles y rangos de N7; en el nivel de equipo se ignoran con diagnóstico (lo hace ya el validador de niveles). Modo de ahorro: 300 s y 180 min. Mientras el daemon no lea la configuración de motor (US-GRP-013), se usan los valores por defecto | N7 |
| D13 | **Arranque**: un repo cuya última actividad supera el umbral arranca dormido: se reconcilia una vez para tener vistas (no hay estado previo en memoria), registra sus vigilancias y su huella, publica `dormant` y cierra su almacén. **Pendiente** la variante barata de N1 (comparar la huella persistida sin reconciliar), que necesita persistir la huella | N1 |

## 3. Estructura

```text
crates/core/src/watch/
  mod.rs        Tier, RepoEntry.tier/dormant, sleep_repo, wake_repo, route → sentinel
  ignore.rs     IgnoreCache (movido de worktree.rs, sin cambios)
  sweep.rs      huella de metadatos, barrido y reconciliación lenta con presupuesto
  repo.rs       run(): flush inicial al despertar con la RefsView guardada
  worktree.rs   flush final al parar por dormir
crates/core/src/profile/store.rs   GapCause::Dormant
crates/core/src/daemon/
  tiers.rs      decisión de dormir, disparadores, Control::Wake
  repos.rs      observed(): última actividad; abrir/cerrar almacén
crates/api/src/{messages.rs,resources.rs}   tier y bloque observation
crates/policy/src/settings/model.rs + schema  engine.observation.*
crates/core/tests/observe_tiers.rs           tests del observador
crates/core/tests/daemon_tiers.rs            tests de extremo a extremo (daemon)
```

## 4. Plan de pruebas (criterio → test)

Todo con repos y perfiles temporales; esperas por señal, sin `sleep` fijos. Las transiciones se provocan con métodos de test (`sleep_repo`, `sweep_now`) para que sean deterministas.

| Criterio (TS-GRP-006) | Test |
|---|---|
| Un dormido conserva sus vigilancias y suelta sus tareas | `observe_tiers::dormant_keeps_watches_and_stops_tasks` |
| Centinela: una edición despierta el repo y queda publicada sin hueco | `observe_tiers::an_edit_wakes_a_dormant_repo_without_a_gap` |
| Una ráfaga produce un único despertar | `observe_tiers::a_burst_wakes_once` |
| Un cambio en una carpeta ignorada no despierta | `observe_tiers::an_ignored_write_does_not_wake` |
| Reflog: tres commits seguidos en un dormido → tres eventos en orden | `observe_tiers::commits_while_dormant_are_three_events_in_order` |
| Barrido sin procesos `git` (shim que cuenta, 100 ciclos → 0) | `observe_tiers::the_sweep_spawns_no_git` |
| Red de seguridad: con el centinela desactivado, un commit lo encuentra el barrido en un hueco `dormant` | `observe_tiers::the_sweep_finds_a_commit_the_sentinel_missed` |
| Red de seguridad: una edición sin `git add` la encuentra la reconciliación lenta | `observe_tiers::the_slow_reconcile_finds_an_edit` |
| Presupuesto: el intervalo efectivo se alarga si no cabe | unidad en `sweep.rs` |
| **NFR-01 (Q49)**: edición seguida de `reset --hard` en un dormido → el contenido queda en la Time Machine | `daemon_tiers::edit_then_reset_hard_in_a_dormant_repo_is_recoverable` |
| Almacén cerrado: un dormido no tiene abierto ningún archivo de su almacén | `daemon_tiers::a_dormant_repo_has_its_store_closed` |
| Despertar por sesión, por petición sobre el repo y por hook; una suscripción a la flota no despierta | `daemon_tiers::wake_triggers` |
| Escrituras durante el despertar quedan publicadas | `daemon_tiers::writes_while_waking_are_published` |
| Un worktree degradado impide dormir | `daemon_tiers::a_degraded_worktree_keeps_the_repo_active` |
| Configuración: claves en el nivel de equipo ignoradas; ahorro alarga intervalos | `crates/policy` (validador de niveles) y unidad de `TierConfig` |
| Rendimiento RES-11/RES-12 | Banco `tiered-scale` (INF-GRP-002), nunca en tests de debug |

## 5. Estado de la entrega (2026-10-07)

**Interruptor**: `DaemonConfig.tiers.dormant_after` (`TierConfig`) es `None` por defecto, así que en producción ningún repo duerme todavía. Los tests lo activan con un umbral corto. **Decisión del coordinador (2026-10-07)**: nada puede dormir un repo en producción hasta que el test de NFR-01 esté en verde en el mismo PR; está en verde, y el interruptor se enciende cuando el tramo 3 lea `dormantAfterHours`.

| Tramo | Contenido | Estado |
|---|---|---|
| 1 · Observador | D1 a D6, D8: `Tier`, `sleep_repo`, `wake_repo`, centinela, huella y barrido, `GapCause::Dormant` | Hecho. `observe_tiers` (6 tests) |
| 2 · Daemon | D9, D10: umbral sin sesión presente, almacén cerrado, despertar por centinela, barrido, sesión, `events.history`, `RawEvents` (undo de la Time Machine), `guard.*` | Hecho. `daemon_tiers` (3 tests, NFR-01 incluido, comprobado con una mutación que apaga el centinela) |
| 3 · Resto | D7 (reconciliación lenta con presupuesto), D11 (contrato `tier` y bloque `observation`), D12 (claves `engine.observation.*`), D13 (arranque dormido), despertar por suscripción a un repo, tests de shim de `git` y de worktree degradado | Pendiente |

## 6. Pendientes

- **Entrega por tramos.** Esta spec se implementa en más de un PR. Lo que queda fuera de cada uno se anota en su descripción.
- **Arranque barato (D13)**: comparar la huella persistida sin reconciliar necesita persistirla; hasta entonces el arranque reconcilia una vez.
- **Lectura de la configuración del motor** (US-GRP-013): hasta entonces, valores por defecto.
- **Escenario `tiered-scale`** del banco (INF-GRP-002): fuera de alcance de la TS; queda declarado.
- **Linux y Windows**: **Pendiente: etapa de validación multiplataforma**. En macOS, el coste de los streams de FSEvents inactivos es un supuesto (Discrepancia de la Enmienda).
