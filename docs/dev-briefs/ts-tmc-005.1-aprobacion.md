---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/ts-tmc-005.2-plan.md"
---

Lee este fichero y luego `ts-tmc-005.2-plan.md`; `ts-tmc-005.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-09T23:43:45.869Z`
- Sha de aprobación: `dbb994b6e313ccd23143437d1e27346d9df0a4adb64903209058941f47f1b7ad:754e55049ec485ae4c074dc991bfa0dd75126c76a6597541cef80f778bcbc519:1908c50e91fad2f9badae357dcace44812d24a626ed1b71e286c9bbfc43020c3`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 7b08ada5b4d2>>>
```text
intent: Que, cuando falta el punto base B de un cambio de agente pero su captura ya está en camino, el undo espere esa captura con un plazo propio y una señal explícita, para que overlap-unverified (D7 de DS-US-TMC-012) quede para los casos en que B de verdad no se puede obtener y no para una carrera con la captura continua.
endState: Un agente que hace raptor undo justo después de un git reset --hard crudo recupera su trabajo a la primera, también en una máquina cargada, sin que el undo deshaga más de lo que deshace hoy tras un reintento: la captura continua publica por repo cada captura sellada o abandonada, el planificador (undo simple, --since y undo por agente) espera como máximo BASE_WAIT sin sleep ni sondeo y sin el lock del oplog ni el writer del almacén, vuelve a leer y aplica base_point_of sin cambios; sin captura pendiente, sin motor o con referencia sin atribuir no espera; al vencer responde overlap-unverified sin cambios en el repo; el daemon registra tm_base_wait con base_wait_ms y result.
nonGoals:
  - cambiar D7 (sin B sigue fallando cerrado)
  - usar el previo como base
  - atribuir ediciones (G2)
  - acelerar la captura continua
  - el solape de la restauración (US-TMC-024)
  - quitar los sleep ya existentes en crates/core/src/timemachine fuera del camino de la espera
  - espera en el redo (su base es el ancla síncrona)
```
<<<FIN-DATOS-NO-CONFIABLES 7b08ada5b4d2>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 4308279b747d>>>
```text
[1] id: TMC005-CA1
statement: CA-1: un agente que hace raptor undo justo tras un git reset --hard crudo, sin esperar la captura, recupera su trabajo a la primera (undo simple y undo por agente, con la captura retenida 500 ms por CaptureLayer; e2e sin la espera previa al undo)
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca1 && cargo test -p gitraptor-cli --test raw_git_undo an_agent_recovers_its_work_after_a_raw_reset_hard && cargo test -p gitraptor-cli --test undo_overlap_same_session
tests:
  - crates/core/src/timemachine/base_wait_tests.rs
  - apps/cli/tests/raw_git_undo.rs
  - apps/cli/tests/undo_overlap_same_session.rs

[2] id: TMC005-CA2
statement: CA-2: con la captura retenida más que BASE_WAIT (5 s), el undo responde overlap-unverified en <= BASE_WAIT + OVERLAP_BUDGET, sin cambios en el repo y con el rechazo en el oplog
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca2
tests:
  - crates/core/src/timemachine/base_wait_tests.rs

[3] id: TMC005-CA3
statement: CA-3: sin captura pendiente (abandonada por MAX_RETRIES, o sin motor) no hay espera: overlap-unverified en menos de 100 ms más el plan, con tm_base_wait result=not-pending cuando hay motor
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca3
tests:
  - crates/core/src/timemachine/base_wait_tests.rs

[4] id: TMC005-CA4
statement: CA-4: con referencia sin atribuir no se espera nunca: undo sin atribuir con la captura retenida, sin tm_base_wait con base_wait_ms > 0
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca4
tests:
  - crates/core/src/timemachine/base_wait_tests.rs

[5] id: TMC005-CA5
statement: CA-5: la espera no retiene el lock del oplog ni el writer del almacén: la captura sella mientras el undo espera, y el lock del oplog está libre durante la espera
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca5
tests:
  - crates/core/src/timemachine/base_wait_tests.rs

[6] id: TMC005-CA6
statement: CA-6: B obtenido tras esperar es el mismo que obtiene un reintento posterior (mismo snapshot_id de B sobre el mismo historial; NFR-01, no deshace de más)
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca6
tests:
  - crates/core/src/timemachine/base_wait_tests.rs

[7] id: TMC005-CA7
statement: CA-7: sin sleep en el camino de producción de la espera (sustituye el clippy/grep de CI de la TS, G4 del Brief): base_wait.rs entero y el tramo entre los marcadores '// base-wait: begin' y '// base-wait: end' de undo.rs y undo_agent.rs (cada marcador exactamente una vez por fichero, en orden y con un tramo no vacío) no contienen sleep, park_timeout ni try_recv
verify: cargo test -p gitraptor-core --lib timemachine::base_wait::tests::ca7
tests:
  - crates/core/src/timemachine/base_wait_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES 4308279b747d>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES a5d30b734aeb>>>
```text
- **G1 — Dónde se espera respecto al `repo_lock`. Decisión del orquestador (2026-11-09), validada por Arquitecto (architecture-advisor).** Se espera **con el `repo_lock` tomado** (`undo.rs:1233`), sin el lock del oplog (`plan` ya lo suelta en `undo.rs:720`) ni el writer del almacén (solo lo toma el aplicador, después). Razones: la captura continua no toma el `repo_lock` (ningún uso en `store/`, `continuous.rs` ni `daemon/modules/tm_capture.rs`); el conjunto que se deshace solo se conoce bajo ese lock (`undo.rs:587`, `undo_agent.rs:177`), y soltarlo dejaría cambiar ese conjunto y obligaría a canjear dos veces el desafío de confirmación (`confirm.gate_over`, `undo.rs:746`). Coste aceptado: una operación encolada en el repo (`executor/mod.rs:947`) espera hasta `BASE_WAIT` más, acotado por CA-2, y `repo_lock::busy()` (`daemon/profile_loss.rs:25`) ve el repo ocupado ese tiempo. CA-5 prueba que no hay interbloqueo. La TS § Alcance Técnico «Dónde se espera» y ADR-TMC-005 § Nota (2026-11-08) se enmiendan en T006.
- **G2 — Corrección de las notas: la señal se indexa por `repo_id` y `seq`, no por ruta de worktree.** El `seq` del evento es único por repo (`store.last_seq`, `daemon/repos.rs:567`). Así el planificador no tiene que comparar la ruta que usa el undo con la que firma el evento. `CaptureSeals` guarda, por repo: el `seq` máximo señalado, los `seq` señalados sin asentar (cada uno con su worktree) y si la captura corre.
- **G3 — Ventana de traspaso.** El evento queda persistido y visible para el undo (`set_persisted`, `repos.rs:567`) antes de que llegue la señal (`modules.git_event`, `repos.rs:659`). Si `seq` > máximo señalado del repo y la captura corre, se espera la señal hasta `HANDOFF` = 100 ms (condvar). Si no llega: `not-pending`.
- **G4 — CA-7, alcance del grep.** Un grep de `sleep` sobre todo `crates/core/src/timemachine` falla hoy por código ajeno a esta TS (`engine.rs:717`, `engine.rs:753`, `continuous.rs:746`, `timeline.rs:1513`, `oplog/recovery.rs:461`), y el workspace no tiene `disallowed-methods` (`Cargo.toml:49`). Por eso CA-7 se verifica con un test, `ca7_*` en `crates/core/src/timemachine/base_wait_tests.rs`, que **sustituye** la verificación por clippy o grep de CI que propone la TS (§ Plan de Verificación, CA-7). El test lee con `include_str!` tres fuentes: `base_wait.rs` entero y, en `undo.rs` y `undo_agent.rs`, el texto entre los comentarios marcadores de nombre fijo `// base-wait: begin` y `// base-wait: end`. Esos marcadores encierran el bloque de cada planificador que llama a `missing_agent_bases` y `await_bases`. El test exige que cada marcador aparezca exactamente una vez por fichero y en ese orden, y que cada tramo no esté vacío, así que no puede pasar sin leer nada. Falla si alguno de los tres textos contiene `sleep`, `park_timeout` o `try_recv`. La espera usa solo `Condvar::wait_timeout`. Los `sleep` citados quedan fuera de alcance (Not Built).
- **G5 — Plazos.** `BASE_WAIT` (2 s, ⚠️ ASSUMPTION de la TS) va fuera de `OVERLAP_BUDGET` (`undo.rs:207`). La planificación corre el `deadline` de fase 1 lo esperado y devuelve `Planned.waited`. Fase 2 recibe `OVERLAP_BUDGET − (transcurrido − waited)` (`undo.rs:1371`).
- **G6 — El redo no espera.** Su `undone` es `OpRef::Oplog` (`redo.rs:306`) y su base es el ancla síncrona. `missing_agent_bases` devuelve vacío y `redo.rs` no cambia. Cumple «un solo camino» de la TS: la función que decide vive junto a `base_point_of`.
- **G7 — e2e.** `undo_overlap_same_session.rs` conserva `captured_by_event(reset.seq)` (`:360`): sirve a G4 de DS-US-TMC-012 (una edición posterior no puede caer en B), no al undo. Se quita solo `captured_after_event(moved.seq)` (`:403-404`) y su helper si queda sin uso. En `raw_git_undo.rs` se quita `m.captured_all(..)` (`:386`) y su comentario (`:380-385`): el undo del desarrollador sin confirmar sale en la puerta de confirmación (`undo.rs:745`), antes de cualquier espera.
- **Supuesto S1:** si el hilo de captura muere por un pánico, `seals` sigue con `running`. El waiter vence por plazo (`expired`) y no se cuelga. No se añade un guard de pánico.
- **Coordinación:** TD-TMC-003 (rama `fix/TD-TMC-003-agent-overlap-echoes`) toca `undo_agent.rs` y `overlap.rs` (`check_selected_tail`, `branch_mover`). Las zonas no se solapan con T004 (`undo_agent.rs:225-233`, `overlap.rs:430`), pero el PR se rebasa sobre `main` cuando TD-TMC-003 entre.
```
<<<FIN-DATOS-NO-CONFIABLES a5d30b734aeb>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 65782c5e3543>>>
```text
docs/dev-briefs/ts-tmc-005.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES 65782c5e3543>>>

planSha: `dbb994b6e313ccd23143437d1e27346d9df0a4adb64903209058941f47f1b7ad` (base de la cadena de re-planes)

Léelo en `ts-tmc-005.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 0b946a44737f>>>
```text
docs/dev-briefs/ts-tmc-005.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 0b946a44737f>>>

contractSha: `754e55049ec485ae4c074dc991bfa0dd75126c76a6597541cef80f778bcbc519`

## Tests de criterio

testsSha: `1908c50e91fad2f9badae357dcace44812d24a626ed1b71e286c9bbfc43020c3`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES e32764328d1b>>>
```text
f168f2de79fe9d4ba4703277c19d67c4a733132625827e171183ae8846ca7a27  apps/cli/tests/raw_git_undo.rs
1b894ad6897b935b19a310f9282d56c2bc436f6c67465b1feee9773ca55e67b9  apps/cli/tests/undo_overlap_same_session.rs
73c428feee08d700de8bea8a82b0f0810c354f5697f0b7439c487543b9b3cca8  crates/core/src/timemachine/base_wait_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES e32764328d1b>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 1 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `dbb994b6e313ccd23143437d1e27346d9df0a4adb64903209058941f47f1b7ad`
- contractSha: `754e55049ec485ae4c074dc991bfa0dd75126c76a6597541cef80f778bcbc519`
- testsSha: `1908c50e91fad2f9badae357dcace44812d24a626ed1b71e286c9bbfc43020c3`
- approvalSha: `dbb994b6e313ccd23143437d1e27346d9df0a4adb64903209058941f47f1b7ad:754e55049ec485ae4c074dc991bfa0dd75126c76a6597541cef80f778bcbc519:1908c50e91fad2f9badae357dcace44812d24a626ed1b71e286c9bbfc43020c3`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
