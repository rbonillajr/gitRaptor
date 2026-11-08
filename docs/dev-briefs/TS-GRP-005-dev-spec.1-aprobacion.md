---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/motor-local/dev-specs/TS-GRP-005-dev-spec.md"
---

Lee este fichero y luego `TS-GRP-005-dev-spec.md`; `TS-GRP-005-dev-spec.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-08T22:27:37.816Z`
- Sha de aprobación: `1854cde23c36e049b6d713fd4f05490f73e7fd1d8b2317cb504c6ab5a53e2c3b:c24cb699b1b3841d62874fd0877ec16dc8c76b0ad136193992fb7ab76e85fc95:b76b988e411da68dad37db92831b4c68309b9ba2bedf7bf1c75fa90f7a1e5ae0`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES d51c4c90b734>>>
```text
intent: Que el motor ceda la máquina al usuario: el trabajo de fondo corre con la prioridad del SO de su clase, el camino del snapshot previo nunca espera a un hilo bajado, y en batería o modo de bajo consumo el motor ahorra energía y lo publica (ADR-GRP-015 § 1 y § 2; PR-B de DS-TS-GRP-005, T007 a T014).
endState: En macOS, con `engine.powerSaving` en `auto`, desenchufar el portátil o activar el bajo consumo pone el motor en ahorro en ≤ 60 s. `engine.resources` publica `power_saving` en todos los SO y `pools` en macOS (null fuera de él). La reconciliación periódica pasa de 5 a 15 min y las redes de seguridad de los dormidos se alargan mientras dura el ahorro, y vuelven a su valor al volver a corriente. `raptor-conn`, `raptor-prior` y el hashing de un previo corren en `UserInitiated`, `raptor-tm-capture` en `Default` (D18) y `raptor-tm-seed` en `Background`. Un `git` lanzado por un hilo `Utility` o `Background` sale con nice 5 o 10. Los bancos `tm_snapshot` y `engine` tienen el escenario `--saturated` de RES-07. En Linux y Windows todo compila y degrada sin pánico (XP-45).
nonGoals:
  - Sacar la fase de blobs de la captura fuera del escritor y bajar su hashing a Background (TD-TMC-002, D18)
  - El texto y la vista del modo de ahorro en la TUI y la CLI (US-GRP-019, US-GRP-017)
  - La pausa del predictor (TS-CKP-001)
  - Aplicar clases o leer la fuente de energía en Linux y Windows (XP-45)
  - Cambiar el contrato de `engine.resources` o el perfil `mcp`
```
<<<FIN-DATOS-NO-CONFIABLES d51c4c90b734>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 48f9acf53e09>>>
```text
[1] id: PWR1
statement: Cuando macOS lee la fuente de energía, el motor la traduce a `PowerReading`: el bajo consumo manda sobre la corriente y la batería, sin batería es `Ac` y un fallo es `Unknown`; en macOS `platform_source()` ya no devuelve `Unknown` (T007, guards 7.1 y 7.2).
verify: cargo test -p gitraptor-core --test power_source
tests:
  - crates/core/tests/power_source.rs

[2] id: PWR2
statement: Cuando se pide la lectura de energía en esta Mac, `gitraptor_macsys::power::reading()` devuelve un valor (`on_battery`, `low_power`) sin `unsafe` fuera de `ffi_power` (T007, guard 7.1).
verify: cargo test -p gitraptor-macsys --test power
tests:
  - crates/macsys/tests/power.rs

[3] id: RES1
statement: Cuando el monitor de recursos tiene el modo de energía, `power_saving` sigue al modo (`setting`, `active`) en todos los SO, y `pools` lista solo los pools vivos en macOS y es null donde las clases no están disponibles (T008, guards 8.1 a 8.3).
verify: cargo test -p gitraptor-core --test resources_power
tests:
  - crates/core/tests/resources_power.rs

[4] id: SAV1
statement: Cuando el motor entra en ahorro, la reconciliación periódica pasa a `max(periodic, saving_periodic)` (4 reconciliaciones en 60 min con batería y 12 con corriente), las redes de seguridad de los dormidos se alargan, y al volver a corriente el próximo plazo se recalcula en ≤ 60 s sin disparar una reconciliación (T009, guards 9.1 a 9.3).
verify: cargo test -p gitraptor-core --test observe_power
tests:
  - crates/core/tests/observe_power.rs

[5] id: CLS1
statement: Cuando el motor atiende en macOS, `raptor-conn`, `raptor-prior` y los hilos de hashing de un previo leen `UserInitiated` del SO, la captura continua corre en `Default` y figura como pool `Capture` mientras su hilo vive, y ningún tramo que retiene el escritor corre por debajo de `Default` (T010, D18, guards 10.1 a 10.3).
verify: cargo test -p gitraptor-core --test tm_classes
tests:
  - crates/core/tests/tm_classes.rs

[6] id: CLS2
statement: Cuando se revisa `crates/core/src`, ningún hilo del camino del snapshot previo (`raptor-conn`, `raptor-prior`, `raptor-tm-capture`, `raptor-tm-seed`, los hilos de hashing) nace sin clase declarada, y ningún archivo que toma el escritor del almacén declara `Utility` ni `Background` (T010, guard 6.1 sin las excepciones del PR-B; D11).
verify: cargo test -p gitraptor-core --test work_class_boundary
tests:
  - crates/core/tests/work_class_boundary.rs

[7] id: SEED1
statement: Cuando se siembra el almacén, el hilo `raptor-tm-seed` nace en `Background` por `work_class::spawn_unpooled` (D18; su clase no se puede leer sin un punto de enganche nuevo en producción, así que se fija en el código).
verify: grep -A3 'spawn_unpooled' crates/core/src/timemachine/continuous.rs | grep -q 'WorkClass::Background' && grep -A6 'spawn_unpooled' crates/core/src/timemachine/continuous.rs | grep -q 'raptor-tm-seed'
tests:
  (ninguno)

[8] id: GIT1
statement: Cuando un hilo `Background` o `Utility` lanza `git` por la capa de invocación en macOS, el hijo sale con nice 10 o 5, y el hijo de un hilo `Default` conserva la prioridad heredada (T011, guards 11.1 y 11.2).
verify: cargo test -p gitraptor-git --test child_class
tests:
  - crates/git/tests/child_class.rs

[9] id: GIT2
statement: Cuando se mide el coste de perder `posix_spawn`, existe el banco `spawn_cost` de `crates/git`, que mide `git --version` p50 y p95 con y sin `pre_exec` (T011; la cifra va al PR).
verify: test -f crates/git/benches/spawn_cost.rs && grep -q 'name = "spawn_cost"' crates/git/Cargo.toml && grep -q 'pre_exec\|lower_child' crates/git/benches/spawn_cost.rs
tests:
  (ninguno)

[10] id: BEN1
statement: Cuando se mide RES-07, los bancos `tm_snapshot` y `engine` aceptan `--saturated`, que satura cada núcleo lógico con la carga de `gitraptor_testkit::cpu_load` durante todo el escenario (T013; las cifras van al PR).
verify: test -f crates/testkit/src/cpu_load.rs && grep -q -- '--saturated' crates/core/benches/tm_snapshot.rs && grep -q -- '--saturated' apps/cli/benches/engine.rs && grep -q 'cpu_load' crates/core/benches/tm_snapshot.rs && grep -q 'cpu_load' apps/cli/benches/engine.rs
tests:
  (ninguno)
```
<<<FIN-DATOS-NO-CONFIABLES 48f9acf53e09>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 78bb46eeac54>>>
```text
docs/requirements/features/motor-local/dev-specs/TS-GRP-005-dev-spec.md
```
<<<FIN-DATOS-NO-CONFIABLES 78bb46eeac54>>>

planSha: `1854cde23c36e049b6d713fd4f05490f73e7fd1d8b2317cb504c6ab5a53e2c3b` (base de la cadena de re-planes)

Léelo en `TS-GRP-005-dev-spec.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 16b391f662e9>>>
```text
docs/dev-briefs/ts-grp-005-prb.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 16b391f662e9>>>

contractSha: `c24cb699b1b3841d62874fd0877ec16dc8c76b0ad136193992fb7ab76e85fc95`

## Tests de criterio

testsSha: `b76b988e411da68dad37db92831b4c68309b9ba2bedf7bf1c75fa90f7a1e5ae0`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 35704f26fc82>>>
```text
383e8fdd2790e80e2a534094a07fde71f807ad25cceb4db26aa0bc1ff7925fe7  crates/core/tests/observe_power.rs
1933d2ab163cb53334fd3afc2fa6fc82ded1f1b9bbd37e780becc3481a06a488  crates/core/tests/power_source.rs
f311b50d06f12819848974280e4663c9c843100cf40a6f38e7693ad7c4114c8a  crates/core/tests/resources_power.rs
c19e7d71f74d1e420d0130cb029e779f1214848f29dce46750d8697ac99b5386  crates/core/tests/tm_classes.rs
4faea1f623086a5bc6810496e2246e86befdab89a1e79b79c657800fb10a17dd  crates/core/tests/work_class_boundary.rs
8c129b1968deb8da3a07544f778b7e77822f31cc478c55211b4156271592a2fc  crates/git/tests/child_class.rs
87364475417ebdc0cabcb77b765ab9b100a3180cff04fa6884b2a70c8941af2c  crates/macsys/tests/power.rs
```
<<<FIN-DATOS-NO-CONFIABLES 35704f26fc82>>>

## Crítico

- Rondas (plan): 3
  - ronda 1: AJUSTES · 3 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
  - ronda 3: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `1854cde23c36e049b6d713fd4f05490f73e7fd1d8b2317cb504c6ab5a53e2c3b`
- contractSha: `c24cb699b1b3841d62874fd0877ec16dc8c76b0ad136193992fb7ab76e85fc95`
- testsSha: `b76b988e411da68dad37db92831b4c68309b9ba2bedf7bf1c75fa90f7a1e5ae0`
- approvalSha: `1854cde23c36e049b6d713fd4f05490f73e7fd1d8b2317cb504c6ab5a53e2c3b:c24cb699b1b3841d62874fd0877ec16dc8c76b0ad136193992fb7ab76e85fc95:b76b988e411da68dad37db92831b4c68309b9ba2bedf7bf1c75fa90f7a1e5ae0`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
