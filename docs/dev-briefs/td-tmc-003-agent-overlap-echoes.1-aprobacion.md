---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/td-tmc-003-agent-overlap-echoes.2-plan.md"
---

Lee este fichero y luego `td-tmc-003-agent-overlap-echoes.2-plan.md`; `td-tmc-003-agent-overlap-echoes.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-09T15:55:29.434Z`
- Sha de aprobación: `99e3b0b8fed32d182278019b88cd56273939fac90d43a2326b6e451f8270e31c:fb405921911f470a4927ebf9e4b73eccdb04343188343c59e04d6d9274875be3:c15e68176fd13381d9dee53fe55adcca97c8ce609fc6dd2d31b488ca5c985119`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES b36e55de4ec1>>>
```text
intent: undo --agent no debe contar el eco de un undo o redo de GitRaptor como movimiento ajeno
endState: Tras su propio undo y redo, un segundo undo --agent del mismo agente se completa; un movimiento de otro actor o un eco de GitRaptor de trabajo ajeno sigue dando overlap
nonGoals:
  - No cambia la base del solape (TS-TMC-005), la regla de rutas del undo por agente ni el undo simple
  - No relaja el solape: todo movimiento sin explicar o de otro actor sigue dando overlap
  - No lee eventos de otros worktrees en el undo por agente (hueco previo anotado aparte)
```
<<<FIN-DATOS-NO-CONFIABLES b36e55de4ec1>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 8b09248115a0>>>
```text
[1] id: C1
statement: Con el motor activo, claude-1 deshace su trabajo por agente, lo rehace y vuelve a deshacerlo por agente: el segundo undo se hace, toma los mismos undone_operation_ids que el primero y deja HEAD sin el trabajo
verify: cargo test -p gitraptor-core --test td_tmc_003 an_agent_undoes_its_work_again_after_redoing_it
tests:
  - crates/core/tests/td_tmc_003.rs

[2] id: C2
statement: La regla de ramas descarta solo el eco anclado de un undo o redo Finished que movió únicamente trabajo del conjunto; el eco de una operación sobre trabajo ajeno cuenta con el solicitante de esa operación como actor, y un eco sin ancla, no terminado, en ventana mixta o con actor crudo de otro agente cuenta como movimiento
verify: cargo test -p gitraptor-core --lib timemachine::undo_agent::echo_tests
tests:
  - crates/core/src/timemachine/undo_agent_echo_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES 8b09248115a0>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES 13cde7ab630f>>>
```text
- El motor atribuye las escrituras del daemon como «sin atribuir» (comprobado en el e2e: el conflicto de hoy sale `unattributed`). Si atribuyera el eco a otra sesión, el eco contaría como movimiento (fail-closed).
- Sin cambio: un commit de otro actor en la rama tras la base del agente sigue dando `overlap` (guarda `another_actors_commit_after_an_own_echo_still_stops_the_undo`, verde antes y después) y los tests de `overlap_selected_tests.rs`, `us_tmc_011.rs`, `us_tmc_003*.rs`.
- Fuera de alcance (anotado por el Arquitecto): el undo por agente solo lee eventos del worktree que lo invoca, mientras § 5 habla de eventos de cualquier worktree para las ramas.
- Modo rápido: sin línea base de suite completa; SIN CERTIFICAR. Antes de subir, `cargo test -p gitraptor-core` completo y los e2e de undo/redo de cli.
```
<<<FIN-DATOS-NO-CONFIABLES 13cde7ab630f>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 57b6048ca7f4>>>
```text
docs/dev-briefs/td-tmc-003-agent-overlap-echoes.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES 57b6048ca7f4>>>

planSha: `99e3b0b8fed32d182278019b88cd56273939fac90d43a2326b6e451f8270e31c` (base de la cadena de re-planes)

Léelo en `td-tmc-003-agent-overlap-echoes.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 87b8b65e3c9b>>>
```text
docs/dev-briefs/td-tmc-003-agent-overlap-echoes.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 87b8b65e3c9b>>>

contractSha: `fb405921911f470a4927ebf9e4b73eccdb04343188343c59e04d6d9274875be3`

## Tests de criterio

testsSha: `c15e68176fd13381d9dee53fe55adcca97c8ce609fc6dd2d31b488ca5c985119`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES c44b0029cf2c>>>
```text
961aa1f064e80804cad478ffd574f71281e6678e91862078878988efe539286c  crates/core/src/timemachine/undo_agent_echo_tests.rs
c94ebf52d5d32a5aaa02aa8deb641a624e8f7f8a8ad56faf57ba1df660e141f9  crates/core/tests/td_tmc_003.rs
```
<<<FIN-DATOS-NO-CONFIABLES c44b0029cf2c>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 1 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `99e3b0b8fed32d182278019b88cd56273939fac90d43a2326b6e451f8270e31c`
- contractSha: `fb405921911f470a4927ebf9e4b73eccdb04343188343c59e04d6d9274875be3`
- testsSha: `c15e68176fd13381d9dee53fe55adcca97c8ce609fc6dd2d31b488ca5c985119`
- approvalSha: `99e3b0b8fed32d182278019b88cd56273939fac90d43a2326b6e451f8270e31c:fb405921911f470a4927ebf9e4b73eccdb04343188343c59e04d6d9274875be3:c15e68176fd13381d9dee53fe55adcca97c8ce609fc6dd2d31b488ca5c985119`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
