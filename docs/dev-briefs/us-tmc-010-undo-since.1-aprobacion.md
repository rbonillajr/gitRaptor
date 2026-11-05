---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/us-tmc-010-undo-since.2-plan.md"
---

Lee este fichero y luego `us-tmc-010-undo-since.2-plan.md`; `us-tmc-010-undo-since.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-05T01:25:48.964Z`
- Sha de aprobación: `4ad157fded986c5c9d75b9babe04a6a66df5791426b01fb423f68c488c4baf7e:a5c396c4296f1d20d22625b8c42cb5bac663a7d49538b9c34cba1b20c94cbe32:91ba126dda0f71f53d22d2bc2780a87597ab20b884f41943db1599b9a725f971`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 93abf08f3cdd>>>
```text
intent: Que el desarrollador deshaga de una vez lo ocurrido en su worktree en un periodo (raptor undo --since 20m) como un solo undo recuperable, sin deshacer nunca de más ni saltarse en silencio nada del periodo (NFR-01, D-TMC-29).
endState: timemachine.undo con since deshace el sufijo de la pila del worktree cuyo momento cae en el periodo como una operación undo con un previo propio y Target::Undo(conjunto); UndoResult lleva undone_operation_ids (MCP: undone_count); un periodo inválido da INVALID_PARAMS, uno vacío nothing-to-undo, una operación no deshacible rechaza la petición entera, el trabajo de otro actor pide la confirmación de US-TMC-013; raptor undo --since <duración> [--json] lo expone con textos en/es; agent y operation_id siguen en NOT_IMPLEMENTED.
nonGoals:
  - --agent (US-TMC-011) y el undo por operation_id
  - elegir el periodo en la TUI (apps/cli/src/tui/ sin cambios)
  - redo (US-TMC-003) y on_remote (US-TMC-014, no está en main)
  - seguir un worktree movido por su id (TQ-20)
  - cerrar la historia y xplat-pendientes (T007, lo hace el orquestador)
```
<<<FIN-DATOS-NO-CONFIABLES 93abf08f3cdd>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 5eb3034465cc>>>
```text
[1] id: TMC010-WIRE
statement: T001: UndoResult sin since serializa exactamente la forma de hoy (sin undone_operation_ids) y con since lista todas las operaciones deshechas de la más antigua a la más reciente, con undone_operation_id = la más antigua; McpUndoResult.undone_count = número de deshechas y se omite si es 0; la proyección MCP no lleva ids de Git crudo ni rutas
verify: cargo test -p gitraptor-api --lib timemachine_since_tests
tests:
  - crates/api/src/timemachine_since_tests.rs

[2] id: TMC010-STACK-DONE
statement: T002: UndoStack::done() devuelve las operaciones no deshechas de la más antigua a la más reciente: tras Do(a), Do(b), Do(c), Undo{targets:[c]} es [a, b]; tras un Redo de ese undo vuelve a [a, b, c]; last_operation() no cambia
verify: cargo test -p gitraptor-core --lib timemachine::oplog::stack::done_tests
tests:
  - crates/core/src/timemachine/oplog/stack_done_tests.rs

[3] id: TMC010-SELECT
statement: T003 (D2): con Selection::Since el conjunto es el sufijo de la pila del worktree con momento (recorded_ms del oplog, observed_ms del evento crudo) >= since_ms, en orden de la pila; el recorrido para en la primera operación más antigua aunque haya otra dentro del periodo debajo; Selection::Last da un vector de uno y el comportamiento de hoy
verify: cargo test -p gitraptor-core --lib -- --exact timemachine::undo::since_tests::since_takes_the_suffix_of_the_stack_inside_the_period --exact timemachine::undo::since_tests::since_stops_at_the_first_older_operation
tests:
  - crates/core/src/timemachine/undo_since_tests.rs

[4] id: TMC010-NFR01-WHOLE
statement: T003 (D4, NFR-01): un evento crudo del periodo con is_undoable falso rechaza la petición entera con raw-git-not-covered; nada se aplica, no hay previo del undo y el rechazo queda en el oplog con Target::Undo del conjunto
verify: cargo test -p gitraptor-core --lib -- --exact timemachine::undo::since_tests::a_raw_event_not_covered_rejects_the_whole_period
tests:
  - crates/core/src/timemachine/undo_since_tests.rs

[5] id: TMC010-TARGET
statement: T003: el destino de un undo por periodo es el estado previo de la operación más antigua del conjunto (su previo garantizado, o raw_target si es un evento crudo), disponible y verificado
verify: cargo test -p gitraptor-core --lib -- --exact timemachine::undo::since_tests::the_target_is_the_prior_of_the_oldest
tests:
  - crates/core/src/timemachine/undo_since_tests.rs

[6] id: TMC010-SERVED
statement: T004: timemachine.undo con since ya no responde NOT_IMPLEMENTED (pasa por require_attributed y tm_scope_for como el undo sin selectores); con agent sigue NOT_IMPLEMENTED con implemented_by US-TMC-011
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact since_is_served_and_agent_is_still_not_implemented
tests:
  - crates/core/tests/us_tmc_010.rs

[7] id: TMC010-S1-PERIOD
statement: Escenario 1: A fuera del periodo (más de 10 s antes), B y C dentro, since = 8s; tras el undo el worktree está como después de A; undone_operation_ids = [B, C]; prior_snapshot_id existe, disponible y verifica; el oplog tiene un undo con Target::Undo([B, C])
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact since_undoes_the_operations_of_the_period_only
tests:
  - crates/core/tests/us_tmc_010.rs

[8] id: TMC010-S2-OTHER-WT
statement: Escenario 2: commits en feat-pagos dentro del periodo; tras el undo por periodo de feat-login la huella de feat-pagos (worktree y rama) es la misma
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact since_never_touches_other_worktrees
tests:
  - crates/core/tests/us_tmc_010.rs

[9] id: TMC010-S4-NOTHING
statement: Escenario 4: sin operaciones en el periodo responde OPERATION_REJECTED con nothing-to-undo; huella igual; el rechazo queda en el oplog
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact nothing_in_the_period_changes_nothing
tests:
  - crates/core/tests/us_tmc_010.rs

[10] id: TMC010-S5-CONFIRMED
statement: Escenario 5: una operación sin atribuir y trabajo de un agente en el periodo; el solicitante sin atribuir recibe confirmation-required con reto y los dueños; con el token las dos quedan deshechas en un solo undo confirmado
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact another_actor_in_the_period_is_undone_once_confirmed
tests:
  - crates/core/tests/us_tmc_010.rs

[11] id: TMC010-S6-UNCONFIRMED
statement: Escenario 6: sin token la petición se rechaza con confirmation-required y los dueños; huella igual; nada del periodo se excluye en silencio
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact another_actor_in_the_period_without_confirmation_is_rejected
tests:
  - crates/core/tests/us_tmc_010.rs

[12] id: TMC010-EQ-N-UNDOS
statement: D1/D2: en dos repos iguales, un undo --since sobre uno y dos undo seguidos sobre el otro dejan el mismo contenido del worktree, el mismo árbol de HEAD y las mismas puntas de rama
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact since_equals_consecutive_undos
tests:
  - crates/core/tests/us_tmc_010.rs

[13] id: TMC010-REDO-READY
statement: Redo preparado: tras el undo por periodo la pila del worktree ya no tiene B ni C y next_redo es ese undo
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact the_undo_records_every_target_for_redo
tests:
  - crates/core/tests/us_tmc_010.rs

[14] id: TMC010-PRECOND
statement: DS-US-TMC-015 D7: con un rebase en curso el undo por periodo se rechaza con git-operation-in-progress antes de tomar su previo (ningún punto nuevo); huella igual
verify: cargo test -p gitraptor-core --test us_tmc_010 -- --exact since_with_a_rebase_in_progress_is_rejected_without_a_prior
tests:
  - crates/core/tests/us_tmc_010.rs

[15] id: TMC010-CLI-TEXT
statement: T005: undo.done-since, undo.reason.nothing-in-period y undo.invalid-since existen en en y es con sus marcadores (catalogs_match y every_rejection_has_a_message los cubre la suite)
verify: cargo test -p gitraptor-cli --bin raptor -- undo::since_tests
tests:
  - apps/cli/src/undo/since_tests.rs

[16] id: TMC010-E2E-INVALID
statement: e2e (macOS): raptor undo --since veinte con LANG=es imprime el texto de undo.invalid-since en español, sale con fallo y el repo no cambia
verify: cargo test -p gitraptor-cli --test us_tmc_010_process -- --exact undo_since_invalid_says_why_in_es
tests:
  - apps/cli/tests/us_tmc_010_process.rs

[17] id: TMC010-E2E-NOTHING
statement: e2e (macOS): raptor undo --since 5m sin nada en el periodo imprime undo.reason.nothing-in-period con el periodo en inglés y en español, sale con fallo y el repo no cambia
verify: cargo test -p gitraptor-cli --test us_tmc_010_process -- --exact undo_since_with_nothing_says_so_in_en_and_es
tests:
  - apps/cli/tests/us_tmc_010_process.rs
```
<<<FIN-DATOS-NO-CONFIABLES 5eb3034465cc>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES 5a526ceee70e>>>
```text
- **G1 (fuera de § Estructura)**: `apps/cli/tests/protected_process.rs:226` dejará de dar `NOT_IMPLEMENTED` con `since`. Propuesta: cambiar el selector del ejemplo a `{ "agent": "claude-x" }`, que sigue en `NOT_IMPLEMENTED` (US-TMC-011) y conserva la intención del test (pasa la comprobación del solicitante y para en su historia). El build lo confirma ejecutándolo; si sigue verde, no se toca.
- **G2**: `since_equals_consecutive_undos` compara dos repos temporales distintos, así que sus huellas no se comparan tal cual (las rutas difieren). Se comparan el contenido de los ficheros del worktree, el árbol de `HEAD` y las puntas de las ramas.
- **Supuesto S1**: el reto de un periodo muestra en `TmConfirmData.undone_operation_id` la operación más antigua, y en `owners`, todos los dueños (hasta `MAX_CONFIRM_OWNERS`). La CLI de confirmación no cambia (§ T005).
- **Supuesto S2**: el escenario 1 espera por reloj a que `recorded_ms` de A tenga más de 10 s: es la única espera por tiempo (§ T006). Se espera a la condición (`now − recorded_ms > 10 000`), no un `sleep` fijo a ciegas.
- **Supuesto S3**: un rechazo de `since` registra `Target::Undo(conjunto)` y, sin conjunto, `Target::Undo([])` (§ T003).

### NFR-01: deshacer de menos, nunca de más

- El conjunto es el sufijo de la pila con momento ≥ umbral, y el recorrido para en la primera operación más antigua (D2). Con un reloj desordenado se deshace de menos.
- Si una operación del conjunto no se puede deshacer (evento crudo con `is_undoable` falso, operación manipulada o ilegible, destino no disponible), se **rechaza la petición entera**: `raw-git-not-covered` / `target-unavailable`, la huella no cambia y el rechazo queda en el oplog (D4, D-TMC-29). Nunca se salta en silencio.
- El trabajo de otro actor del periodo nunca se excluye. Con un agente como solicitante, la respuesta es `other-actor`. Sin atribuir, pide confirmación con el reto ligado a los ids del conjunto. Por MCP o en Windows, se rechaza.
- Lo que se apoya en esto: `a_raw_event_not_covered_rejects_the_whole_period`, `since_stops_at_the_first_older_operation`, `since_never_touches_other_worktrees` y `another_actor_in_the_period_without_confirmation_is_rejected`.

Cuestión de coste: la spec obliga a 9 tests de integración y a 2 e2e. Con unitarios sobre `undo_selected` y 4 de integración se cubrirían los mismos caminos más barato. Se construyen todos, como pide la spec.
```
<<<FIN-DATOS-NO-CONFIABLES 5a526ceee70e>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 2c58c8d74a6c>>>
```text
docs/dev-briefs/us-tmc-010-undo-since.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES 2c58c8d74a6c>>>

planSha: `4ad157fded986c5c9d75b9babe04a6a66df5791426b01fb423f68c488c4baf7e` (base de la cadena de re-planes)

Léelo en `us-tmc-010-undo-since.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 7efccf2a0139>>>
```text
docs/dev-briefs/us-tmc-010-undo-since.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 7efccf2a0139>>>

contractSha: `a5c396c4296f1d20d22625b8c42cb5bac663a7d49538b9c34cba1b20c94cbe32`

## Tests de criterio

testsSha: `91ba126dda0f71f53d22d2bc2780a87597ab20b884f41943db1599b9a725f971`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 2245fbf688f4>>>
```text
672293e49c55a118c5c51423dbfbbb092c0bababb9230b144b519582dccc8199  apps/cli/src/undo/since_tests.rs
418c5b81494067ccd45c7adf7965ab4a04f72c6d9b68c2d0b9e8b167327c14ca  apps/cli/tests/us_tmc_010_process.rs
318bf1c2536ee58bfc9c37d4ba594ccbb5dd7b415b33994cbb4509f81fc805a4  crates/api/src/timemachine_since_tests.rs
2bb5dcb307c346fec40787928579f2b98c41425d8d21fcfd7ec6eb6044148a0c  crates/core/src/timemachine/oplog/stack_done_tests.rs
34a97c8bf897b45e3de7f41b1a254f377759ca31f0fca68362e468209ddea345  crates/core/src/timemachine/undo_since_tests.rs
44800bbfe81cdb25f95d8c0884ae77b68f90d62889d875ca5c1936aaa3c87a5f  crates/core/tests/us_tmc_010.rs
```
<<<FIN-DATOS-NO-CONFIABLES 2245fbf688f4>>>

## Crítico

- Rondas (plan): 1
  - ronda 1: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `4ad157fded986c5c9d75b9babe04a6a66df5791426b01fb423f68c488c4baf7e`
- contractSha: `a5c396c4296f1d20d22625b8c42cb5bac663a7d49538b9c34cba1b20c94cbe32`
- testsSha: `91ba126dda0f71f53d22d2bc2780a87597ab20b884f41943db1599b9a725f971`
- approvalSha: `4ad157fded986c5c9d75b9babe04a6a66df5791426b01fb423f68c488c4baf7e:a5c396c4296f1d20d22625b8c42cb5bac663a7d49538b9c34cba1b20c94cbe32:91ba126dda0f71f53d22d2bc2780a87597ab20b884f41943db1599b9a725f971`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
