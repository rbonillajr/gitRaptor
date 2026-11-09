---
id: TD-TMC-003
title: "El solape del undo por agente cuenta los ecos anclados de GitRaptor como movimientos sin atribuir"
type: td
status: draft
feature: time-machine
domain: GRP
priority: medium
complexity: small
created: 2026-11-09
updated: 2026-11-09
related:
  adrs: [ADR-TMC-005]
  stories: [US-TMC-011, US-TMC-003, US-TMC-012]
  specs: [DS-US-TMC-011, DS-US-TMC-003]
ado:
  id: null
  url: null
tags: [time-machine, deuda-tecnica, solape, undo-por-agente, redo, nfr-01]
---

## TD-TMC-003: El solape del undo por agente cuenta los ecos anclados de GitRaptor como movimientos sin atribuir

**Valor**: un agente puede deshacer su propio trabajo después de un undo o un redo previo en la misma rama, sin que la Time Machine tome por trabajo ajeno lo que ella misma escribió.

### Descripción

**Como** desarrollador que orquesta agentes
**Quiero** que `raptor undo --agent` reconozca como propios de GitRaptor los movimientos de rama de un undo o redo anterior
**Para** que el undo por agente no se rechace con un solape falso tras un redo

> Dev Spec: no hace falta (cambio acotado a `overlap.rs` / `undo_agent.rs`; los criterios de abajo bastan) | Pendiente

**Origen.** Implementación de US-TMC-003 (redo), PR del redo: el guard ⛔4.2 original (el agente deshace su trabajo, lo rehace y lo vuelve a deshacer) falla en el último paso. Diagnóstico instrumentado: la fila del redo y su ancla tienen la misma forma que las del undo (`kind: redo`, `scope.refs` con la rama, `engine_mark`, observación con `cause_operation` = el redo y `echo_settled`). **Decisión del coordinador (2026-11-09)**: el arreglo cambia la semántica del solape del undo por agente y se encarga aparte; GUARD-4.2 se enmendó para probar el guard sin pasar por el undo por agente.

**Hoy.**

- El undo por agente comprueba el solape con `overlap::check_selected` y `check_selected_tail`. La regla de ramas (`branch_mover`) toma el primer evento crudo posterior a la base del agente que nombra la rama y no está en el conjunto.
- Esos eventos llegan directos de `engine.raw_events` (`crates/core/src/timemachine/undo_agent.rs:400-406` y `check_selected_tail` en `undo.rs`). No se atan a la operación de GitRaptor que los causó, a diferencia del undo simple, que lo hace con `external_events_in` y `events_by_root`.
- Efecto: el eco anclado de cualquier undo o redo anterior en la misma rama cuenta como un movimiento de "sin atribuir", y el undo por agente se rechaza con `overlap` (sujeto: la rama). Es fail-closed: deshace de menos, nunca de más. No hay riesgo NFR-01, pero bloquea un flujo legítimo: undo → redo → undo del propio agente.

### Alcance Técnico

- **Descartar** en `branch_mover`, o en los eventos que `undo_agent` y `check_selected_tail` le pasan, los eventos que `external_events_in` ata a una operación terminada de GitRaptor (`caused_by` presente). Es la opción mínima.
- **Conservar** como conflicto todo movimiento de rama que ningún evento ni operación explica, y todo movimiento de otro actor (D-TMC-34). Nunca se relaja el solape.
- **Fuera de alcance**: cambiar la base del solape (TS-TMC-005), la regla de rutas del undo por agente y el undo simple.

### Plan de Verificación

#### Pruebas Automatizadas

- **Criterio de aceptación (retoma el GUARD-4.2 original de US-TMC-003):** con el motor activo, claude-1 deshace su propio trabajo por agente, lo rehace de inmediato y vuelve a deshacerlo por agente. El segundo undo se hace, toma los mismos `undone_operation_ids` que el primero y devuelve `HEAD` al estado sin el trabajo.
- Un movimiento de la rama que ningún evento ni operación explica tras la base del agente sigue dando `overlap` (o `overlap-unverified` si no se puede comprobar).
- Un commit de otro actor en la rama tras el primer cambio del agente sigue dando `overlap`.
- Los tests de `overlap_selected_tests.rs`, `us_tmc_011.rs` y `us_tmc_003*.rs` siguen en verde.

#### Verificación Manual / Sandbox

- En un repo temporal con un agente simulado: `raptor undo --agent <id>`, `raptor redo` y de nuevo `raptor undo --agent <id>`. El segundo undo se hace y el repo vuelve al estado sin el trabajo del agente.
