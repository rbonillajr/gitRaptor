---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/td-grp-006-fsevents-under-load.2-plan.md"
---

Lee este fichero y luego `td-grp-006-fsevents-under-load.2-plan.md`; `td-grp-006-fsevents-under-load.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-10T11:47:23.199Z`
- Sha de aprobación: `32562067ae7f2761674df112227b3394bc7b3d02f0921dd008428e0bc188dcc3:1447089d1188a81c3af6fccc5d2a6cf3a9f0acd4a0c5835aff4dca4a2c5592d6:3db8786413c8c02be9ad13e82e2d8301161aab565a17a698da7466b25ad4fb4f`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES c75aaf5335dd>>>
```text
intent: Medir en el log cuántas escrituras del directorio Git ve el motor por un evento FS y cuántas solo por una lectura pedida (Wake) o por el sondeo de respaldo, y que us_grp_010 deje de depender de la latencia de FSEvents, sin cambiar la red de seguridad ni la regla S3 (TD-GRP-006, ADR-GRP-010 Enmienda 2026-11-10).
endState: Cada s3_read_notice lleva via=event (lectura por la ventana de un evento FS), via=wake (lectura pedida con poll_repo) o via=backup (temporizador del sondeo o simulate_poll), como palabra fija y sin rutas; the_time_machine_sees_the_current_attribution espera por señales (marcador del hook, lectura forzada con scope.snapshot) y repite el commit como máximo 3 veces si sale sin atribuir.
nonGoals:
  - cambiar el intervalo del sondeo de respaldo o añadir un sondeo rápido
  - cambiar la regla S3 o READ_WRITE_MAX_AGE (ADR-GRP-012)
  - medir la latencia de los eventos del worktree
  - estabilizar los tests de caja blanca del observador (TD-GRP-010)
  - agregados p50/p99 en engine.resources
```
<<<FIN-DATOS-NO-CONFIABLES c75aaf5335dd>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 897592f5e1aa>>>
```text
[1] id: GRP006-VIA-WAKE
statement: Con los eventos FS tirados (simulate_lost_events) y el sondeo de respaldo lejano, un commit en un repo observado seguido de Observer::poll_repo produce git_dir_written con ReadVia::Wake, y ninguno con otro via
verify: cargo test -p gitraptor-core --test td_grp_006 -- via_wake
tests:
  - crates/core/tests/td_grp_006.rs

[2] id: GRP006-VIA-BACKUP
statement: Con los eventos FS tirados: con backup_poll de 1 s, un commit sin poll_repo ni simulate_poll produce git_dir_written con ReadVia::Backup (el temporizador); y en otro observador con el sondeo de respaldo lejano, un commit seguido de Observer::simulate_poll también da ReadVia::Backup, y ninguno con otro via
verify: cargo test -p gitraptor-core --test td_grp_006 -- via_backup
tests:
  - crates/core/tests/td_grp_006.rs

[3] id: GRP006-VIA-EVENT
statement: Con los eventos FS entregados y el sondeo de respaldo lejano, sin poll_repo ni simulate_poll, la lectura que encuentra un commit produce git_dir_written con ReadVia::Event (la escritura se repite contando intentos si el SO pierde el evento); en el mismo observador, tras tirar los eventos, un commit seguido de poll_repo da ReadVia::Wake, así que el test no sale verde con un via constante
verify: cargo test -p gitraptor-core --test td_grp_006 -- via_event
tests:
  - crates/core/tests/td_grp_006.rs

[4] id: GRP006-LOG
statement: SEC-04: ReadNoticeLog escribe s3_read_notice con via=event, via=wake o via=backup según el ReadVia recibido, además de repo, age_ms y kept, y ningún valor de la línea es una ruta
verify: cargo test -p gitraptor-core --lib daemon::sessions::read_notice_tests
tests:
  - crates/core/src/daemon/sessions_read_notice_tests.rs

[5] id: GRP006-US010
statement: the_time_machine_sees_the_current_attribution espera por señal (marcador del hook post-commit, lectura forzada con scope.snapshot, archivo go), repite el commit como máximo 3 veces si sale sin atribuir, imprime age_ms y via de cada intento al fallar, y pasa
verify: cargo test -p gitraptor-core --test us_grp_010 -- --exact the_time_machine_sees_the_current_attribution
tests:
  - crates/core/tests/us_grp_010.rs
```
<<<FIN-DATOS-NO-CONFIABLES 897592f5e1aa>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES 17bdf70b48d0>>>
```text
- **GRP006-US010 no necesita código de producto** para pasar: es un arreglo de test. Su rojo es el medido en local antes del cambio (3 de 10 con carga 9–19, TD-GRP-006 § Diagnóstico). Solo el mensaje de fallo usa `via` (T002).
- **GRP006-VIA-EVENT necesita un evento FS real**: bajo pérdida silenciosa se repite la escritura contando intentos (guarda de cuelgue de 120 s, sin cota de pared en la aserción). Las otras dos tiran los eventos con `simulate_lost_events(true)` y no dependen de FSEvents.
- **Un `Rescan`** (flag de pérdida del SO) abre una ventana y su `flush` sale como `via=event`. Es raro y la ficha no lo separa.
- **El `via` del temporizador en un test**: `backup_poll` corto en `WatchConfig` (1 s) solo en el caso del temporizador de GRP006-VIA-BACKUP. El caso de `simulate_poll` usa otro observador con el sondeo lejano, como VIA-WAKE, para que el temporizador no gane la carrera (PC-2).
- **GRP006-VIA-EVENT con el stub**: el stub pasa siempre `Event`, así que el test añade, en el mismo observador, un paso por `poll_repo` que debe dar `Wake`; sin él saldría verde desde el stub (PC-3).
- **10 de 10 bajo carga** (Plan de Verificación de la ficha, PC-1): no cabe en un `verify` porque la carga no es reproducible. Lo corre el orquestador después de la construcción y antes del PR: `the_time_machine_sees_the_current_attribution` 10 veces seguidas, con `cargo test -p gitraptor-git` corriendo en paralelo, y deja en el PR las cifras y la carga medida. Si no da 10 de 10, el PR lo dice.
- **Modo rápido** (Rene, 2026-11-08): sin línea base de la suite completa. Los tests del contrato se prueban en rojo de forma dirigida y la entrega queda **SIN CERTIFICAR**. Excepción NFR-01, porque toca el observador que alimenta la Time Machine: `cargo test -p gitraptor-core` completo y los e2e de undo de `apps/cli` antes de subir.
- **Linux/Windows**: el `via` es igual en todos los SO; la medición de FSEvents es solo de macOS. Pendiente de la validación multiplataforma.
```
<<<FIN-DATOS-NO-CONFIABLES 17bdf70b48d0>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 618c4cca2a43>>>
```text
docs/dev-briefs/td-grp-006-fsevents-under-load.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES 618c4cca2a43>>>

planSha: `32562067ae7f2761674df112227b3394bc7b3d02f0921dd008428e0bc188dcc3` (base de la cadena de re-planes)

Léelo en `td-grp-006-fsevents-under-load.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 3150b01767ac>>>
```text
docs/dev-briefs/td-grp-006-fsevents-under-load.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 3150b01767ac>>>

contractSha: `1447089d1188a81c3af6fccc5d2a6cf3a9f0acd4a0c5835aff4dca4a2c5592d6`

## Tests de criterio

testsSha: `3db8786413c8c02be9ad13e82e2d8301161aab565a17a698da7466b25ad4fb4f`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES be8dd2360e36>>>
```text
a290eaf82e323f041d37b1916fc9e4e80645cae30d620a9c0521063f16933e6b  crates/core/src/daemon/sessions_read_notice_tests.rs
ef1b3fd4619dd4b59deb91d1781837c1a354096730761cae1e22977b6a1993a2  crates/core/tests/td_grp_006.rs
8160c71eea9e2514f9646eb50997cc305487008c8dc01af4777dc7d4200ad6f1  crates/core/tests/us_grp_010.rs
```
<<<FIN-DATOS-NO-CONFIABLES be8dd2360e36>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `32562067ae7f2761674df112227b3394bc7b3d02f0921dd008428e0bc188dcc3`
- contractSha: `1447089d1188a81c3af6fccc5d2a6cf3a9f0acd4a0c5835aff4dca4a2c5592d6`
- testsSha: `3db8786413c8c02be9ad13e82e2d8301161aab565a17a698da7466b25ad4fb4f`
- approvalSha: `32562067ae7f2761674df112227b3394bc7b3d02f0921dd008428e0bc188dcc3:1447089d1188a81c3af6fccc5d2a6cf3a9f0acd4a0c5835aff4dca4a2c5592d6:3db8786413c8c02be9ad13e82e2d8301161aab565a17a698da7466b25ad4fb4f`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
