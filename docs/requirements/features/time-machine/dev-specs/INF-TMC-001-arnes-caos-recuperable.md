---
id: DS-INF-TMC-001
title: "Dev Spec — Arnés de caos: muerte del daemon y escenarios hostiles con raptor undo"
type: dev-spec
status: partially-implemented
feature: time-machine
domain: GRP
story: INF-TMC-001
created: 2026-10-08
updated: 2026-10-08
related:
  stories: [INF-TMC-001, US-TMC-019, US-TMC-018, US-TMC-002, US-TMC-004, INF-GRP-001, TS-TMC-001, TS-TMC-002, TS-TMC-003, TS-TMC-004]
  adrs: [ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-005, ADR-GRP-009]
  nfrs: [NFR-01, NFR-12]
tags: [time-machine, caos, nfr-12, nfr-01, recuperacion, undo, puntos-de-fallo, hostiles]
---

# Dev Spec — INF-TMC-001: arnés de caos (primer corte)

Plano compacto de [INF-TMC-001](../technical-stories/INF-TMC-001-arnes-caos-recuperable.md). Une lo que ya está en `main`: la captura con su punto de validez (TS-TMC-001), el diario y la recuperación al arrancar (TS-TMC-002, ADR-TMC-003 § 6), el aplicador con su diario de pasos (TS-TMC-003), la operación protegida (TS-TMC-004), `raptor undo` (US-TMC-002) y la captura continua del Git crudo (US-TMC-004).

**Qué entrega este corte**: el daemon real (el binario `raptor`) **muere de verdad** (`SIGKILL`) en cada transición de un undo y en cada paso del aplicador; al relanzarlo, la recuperación cierra los estados y `raptor undo` devuelve el worktree al estado previo sin perder nada. Y los escenarios hostiles de Git de la ficha, cada uno como un test que demuestra "nada se pierde y `raptor undo` lo recupera". Todo determinista: sin esperas fijas, cada estado se espera por una señal con un plazo.

**Qué no entrega** (queda en la ficha, § 6): el canario de SEC-TMC-02 sobre las escrituras internas, los casos de seguridad (SEC-TMC-04/09/11/12/14: disco lleno, almacén u oplog editados fuera, corpus de rutas), la purga (ADR-TMC-007, aún sin construir), la sensibilidad por defecto sembrado y Windows.

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `crates/core/src/timemachine/chaos.rs` (nuevo) | Puntos de fallo: `POINTS` (lista cerrada) y `crash_point(name)`. Solo en builds de depuración (SEC-06): lee una vez `GITRAPTOR_TEST_TM_CRASH_AT` (un nombre fuera de `POINTS` hace `panic`) y, si nombra ese punto, el proceso se mata con `SIGKILL` (`rustix`, ya dependencia; en Windows, `abort`). En release el cuerpo no se compila (`#[cfg(debug_assertions)]`): ni los nombres ni el `kill` llegan al binario |
| `crates/core/src/timemachine/store/capture.rs` (`record`) | Puntos `prior:pending` (fila `pending` escrita, sin ref) y `prior:ref` (ref creada, fila sin completar). Solo si `level == GuaranteedPrior`: la observación continua y el ancla pasan por el mismo código y no deben disparar (tampoco `HookPrior`) |
| `crates/core/src/timemachine/protected/mod.rs` (`run`) | Puntos `operation:intent`, `operation:prior` y `operation:ready`, tras escribir cada transición del diario |
| `crates/core/src/timemachine/apply/mod.rs` (`step`, `files`, `apply_holding`) | Puntos `apply:step-3` … `apply:step-7`, tras la entrada del diario de cada paso (el mismo sitio que `ApplyHooks::at_step`); `apply:mid-files`, antes del segundo intercambio de archivo (unos archivos escritos y otros no); `operation:applied`, con `run_steps` en `Ok` y antes del paso 8 (locks sin liberar, operación sin cerrar). No va en `protected::run`: el undo anota sus propios pasos (`self_annotated`) y el aplicador ya lo cierra como `finished` |
| `apps/cli/tests/tm_chaos.rs` (nuevo) | La suite: daemon real como proceso hijo, repo, home y perfil temporales (NFR-01). Un test por punto (`crash_at_*`), un test de cobertura que falla si un punto de `POINTS` no tiene escenario y los escenarios hostiles (`hostile_*`) con el mismo arnés |

## 2. Puntos de fallo — D1

Decisión del orquestador (2026-10-08), validada por el Arquitecto (aprobó con ajustes, todos incorporados: `operation:applied` dentro del aplicador, filtro `GuaranteedPrior`, cuerpo fuera de release, `panic` ante un nombre desconocido, comprobación de la señal 9 y de los locks propios en Linux, y espera de la captura antes del segundo undo): los puntos son **nombres estables en una lista cerrada** y se activan por una variable de entorno **solo en depuración**, igual que los demás ganchos de test del daemon (`GITRAPTOR_TEST_*`, SEC-06). La muerte es `SIGKILL` sobre el propio proceso, no un `panic`: no corre ningún `Drop`, así que los locks propios y las filas a medias quedan como tras un corte real, y es la recuperación (ADR-TMC-003 § 6) la que tiene que cerrarlos. `ApplyHooks` sigue siendo el mecanismo de los tests en proceso; los puntos de este corte son para el daemon como proceso.

| Punto | Estado en disco al morir | Lo que la recuperación debe dejar |
|---|---|---|
| `operation:intent` | undo `intent`, sin previo | undo `aborted`; worktree sin cambios |
| `prior:pending` | snapshot previo `pending`, sin ref | snapshot `discarded`; undo `aborted`; sin cambios |
| `prior:ref` | snapshot `pending` con ref | ref borrada, snapshot `discarded`; undo `aborted`; sin cambios |
| `operation:prior` | undo `prior-snapshot` | undo `aborted`; sin cambios |
| `operation:ready` | undo `ready` | undo `aborted`; sin cambios |
| `apply:step-3` … `apply:step-7` | undo `applying(n)`, locks propios tomados | undo `interrupted`, un aviso por worktree, locks propios liberados |
| `apply:mid-files` | `applying(6)`, archivos a medias | igual que arriba |
| `operation:applied` | aplicado, sin cerrar | igual que arriba |

## 3. El escenario de muerte — D2

1. Repo temporal con un worktree enlazado `feat-login` observado (perfil temporal, alta por la API del perfil). Daemon hijo con la captura continua.
2. Trabajo sin commitear (`login.txt` modificado y `util.rs` nuevo): se espera a que una observación lo capture (estado **S0**).
3. `git reset --hard` crudo: el trabajo se pierde (estado **S1**). Se espera el evento `reset` del motor.
4. Se relanza el daemon con `GITRAPTOR_TEST_TM_CRASH_AT=<punto>` y se pide `raptor undo`: el cliente falla y el daemon muere por `SIGKILL` (la señal es la prueba de que el punto se ejecutó).
5. Se relanza el daemon sin la variable. La recuperación debe dejar lo de la tabla de § 2.
6. Puntos anteriores a `applying`: el worktree es **S1** byte a byte y `raptor undo` devuelve **S0**.
7. Puntos de `applying` y `operation:applied`: se espera una captura del worktree posterior al arranque (el undo interrumpido no tiene ancla y su eco llega hasta ella). `raptor undo` devuelve el worktree a **S1** (el previo del undo interrumpido, ADR-TMC-003 § 6.3) y el siguiente `raptor undo` devuelve **S0**: nada se pierde. Hay un único aviso pendiente de interrupción para el worktree y ningún lock propio queda en el repo.

"Estado" es lo que ve el usuario: archivos del worktree, índice (`ls-files -s`), `HEAD`, refs de ramas y `git status`.

## 4. Escenarios hostiles — D3

Mismo arnés, sin puntos de fallo. Cada uno parte de S0 → `reset --hard` crudo → S1 y termina con `raptor undo` → S0.

| Escenario | Qué hace el "agente" antes del undo | Qué se comprueba además |
|---|---|---|
| Push de todo | `git push --mirror` a un remoto temporal (bare) | El remoto no tiene ninguna ref ni objeto de los snapshots (ni la ref, ni el blob de `util.rs`) |
| Mantenimiento agresivo | `reflog expire --expire=now --all` + `gc --prune=now --aggressive` tras un `reset --hard HEAD~1` que tira un commit | La rama y el commit vuelven (los objetos salen del almacén) |
| Agente que limpia y resetea | `git clean -fd` + `reset --hard` | Vuelven los archivos sin seguimiento no ignorados. Los ignorados no se capturan por diseño (US-TMC-001) y no se prometen |
| Lock de Git ajeno | `index.lock` del worktree y `refs/heads/feat-login.lock` creados por otro programa | El undo se rechaza (`git-busy`), nada cambia y el lock ajeno sigue; al quitarlo, el undo recupera |
| Operación de Git a medias | `merge` con conflicto (`MERGE_HEAD`) | El undo se rechaza (`git-operation-in-progress`) y nada cambia; tras `merge --abort`, el undo recupera |

## 5. Plataformas — D4

`#![cfg(any(target_os = "macos", target_os = "linux"))]`, como `us_tmc_002` y `us_tmc_004`: sin `script` ni nada propio de macOS. Windows queda fuera: el CI de Windows no puede correr tests con Git todavía (TS-GRP-002 rechaza todo Git en Windows hasta tener la comprobación de ACL) y `SIGKILL` no existe. Pendiente: etapa de validación multiplataforma.

La suite corre en el paso general `cargo test` del CI, bloqueante en macOS y Linux.

## 6. Fuera de este corte (pendiente en la ficha)

- Canario de SEC-TMC-02 sobre las escrituras internas (monitor del sistema de archivos, `core.worktree` forzado, `includeIf`, filtros, firma global, plantillas con hooks).
- Casos de seguridad: escritura concurrente durante el intercambio atómico, disco lleno con reserva del previo, almacén u oplog editados fuera del daemon, corpus de rutas hostiles y refs con opciones (SEC-TMC-04, 09, 11, 12, 14). En parte ya cubiertos por `tm_apply` y `tm_store_safety` en proceso.
- Puntos de la purga (ADR-TMC-007 § 4): la purga no está construida.
- Muerte durante una captura de observación y durante una restauración a un punto del timeline (US-TMC-009, sin construir).
- Sensibilidad (defecto sembrado), informe de cobertura como artefacto del CI y semilla: este corte no tiene aleatoriedad, así que dos ejecuciones dan lo mismo por construcción.
- Windows (§ 5) y la verificación manual con 10 worktrees en la máquina de dogfooding.

## 7. Verificación

| Criterio | Comando |
|---|---|
| Cada punto mata el daemon y la recuperación deja lo de § 2; `raptor undo` vuelve a S1 y luego a S0 | `cargo test -p gitraptor-cli --test tm_chaos crash_at_` |
| Ningún punto de `POINTS` queda sin escenario | `cargo test -p gitraptor-cli --test tm_chaos every_crash_point_has_a_scenario` |
| Los escenarios hostiles de § 4 | `cargo test -p gitraptor-cli --test tm_chaos hostile_` |
| Sin regresiones | `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` |

## Estado de la implementación (2026-10-08)

Implementado en: PR #189. Verificado en macOS (los 18 tests de `tm_chaos`, tres ejecuciones seguidas, y `cargo test --workspace`); Linux, en el CI.

Decisiones tomadas al implementar:

- **Ajustes del coordinador** (2026-10-08): los puntos van tras la feature `chaos` de `gitraptor-core` (no tras `debug_assertions`: los tests en release con aserciones de depuración la dejarían viva), que solo activa la dev-dependency de `gitraptor-cli`; `crates/core/tests/tm_chaos_gate.rs` comprueba que no es `default`, que solo la activan dev-dependencies y que sin ella `crash_point` está vacía. Cada escenario compara el estado completo (bytes de los archivos, índice, `HEAD`, ramas y `git status`) tras la recuperación y tras cada undo.
- **Señal 9**: el `SIGKILL` a uno mismo puede llegar después de que `kill` vuelva; el proceso se queda esperándolo (`park`) en lugar de llamar a `abort`, que competía con él y moría por `SIGABRT`.
- **Espera de la captura antes del segundo undo** (ajuste 6 del Arquitecto): no hace falta. Tras el arranque no hay ninguna captura nueva del worktree a medias, y el segundo undo apunta al undo interrumpido (lo comprueba `undone_operation_id`), no a un evento crudo; el previo que toma ese undo cubre el estado a medias.
- **`merge --abort` es Git crudo**: un reset a `HEAD`. El primer undo tras abortar lo deshace y deja S1 (un merge en curso nunca se restaura), y el siguiente devuelve S0.

Hallazgos sin pérdida de datos (anotados en el PR, fuera de este corte):

- Con un `index.lock` ajeno el undo se rechaza como `repo-busy` ("another operation is running"), no como `git-busy`: el motor no se calma con el índice bloqueado y el undo no llega a las precondiciones del aplicador. No cambia nada y el lock sigue, pero el mensaje es menos preciso.
- Lo que el aplicador escribió antes de morir no queda capturado por ninguna observación al arrancar; lo cubre el previo del siguiente undo.
