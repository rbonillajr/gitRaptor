---
id: US-TMC-010
title: "El desarrollador deshace todo lo ocurrido en su worktree en los últimos minutos"
type: us
status: implemented
priority: medium
created: 2026-10-03
updated: 2026-11-04
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-TMC-012, US-TMC-013]
  specs: [DS-US-TMC-010]
covers: [BR-TMC-WF-001, BR-TMC-WF-002, BR-TMC-VAL-001, BR-TMC-AUTH-001, D-TMC-19, D-TMC-23, D-TMC-29]
blocked_by: []
tags: [time-machine, undo, since]
---

# US-TMC-010: El desarrollador deshace todo lo ocurrido en su worktree en los últimos minutos

## Descripción

**Como** desarrollador orquestador
**Quiero** usar `raptor undo --since <duración>` para deshacer todas las operaciones del worktree actual en ese periodo
**Para** revertir de una vez una racha de cambios equivocados sin elegirlos uno a uno

**Valor**: "deshaz lo de los últimos 20 minutos" en un solo comando (BR-09).

## Reglas cubiertas

BR-TMC-WF-001 (ámbito: el worktree desde el que se invoca) · BR-TMC-VAL-001 (periodo inválido o vacío) · BR-TMC-WF-002 (undo por periodo sin `--agent`: cualquier actor, sin exclusiones silenciosas, D-TMC-29) · BR-TMC-AUTH-001 (operaciones de otro actor, D-TMC-23) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Deshacer el periodo indicado**

Dado en "feat-login" dos operaciones de un solicitante que queda sin atribuir en los últimos 20 minutos y una anterior
Cuando ese solicitante pide deshacer desde "feat-login" lo ocurrido en los últimos 20 minutos
Entonces se deshacen las dos operaciones del periodo
  Y la operación anterior sigue aplicada
  Y existe un punto recuperable previo al undo

**Escenario: Otros worktrees no cambian**

Dado operaciones en "feat-pagos" en los mismos 20 minutos
Cuando un solicitante sin atribuir pide deshacer desde "feat-login" los últimos 20 minutos
Entonces "feat-pagos" no cambia

**Escenario: Periodo no válido**

Dado el worktree "feat-login"
Cuando un solicitante sin atribuir pide deshacer con el periodo "veinte"
Entonces el repo no cambia
  Y el solicitante recibe el motivo

**Escenario: Nada que deshacer en el periodo**

Dado que "feat-login" no tiene operaciones en los últimos 20 minutos
Cuando un solicitante sin atribuir pide deshacer ese periodo
Entonces el repo no cambia
  Y el solicitante recibe el aviso de que no hay operaciones en ese periodo

**Escenario: Con trabajo de otro actor en el periodo y confirmación, se revierte**

Dado en "feat-login" una operación sin atribuir y un commit de "claude-1" en los últimos 20 minutos
  Y el solicitante del undo queda sin atribuir
Cuando pide deshacer los últimos 20 minutos
  Y lo confirma de forma interactiva
Entonces se deshacen las dos operaciones

**Escenario: Con trabajo de otro actor en el periodo y sin confirmación, se rechaza**

Dado en "feat-login" una operación sin atribuir y un commit de "claude-1" en los últimos 20 minutos
  Y el solicitante del undo queda sin atribuir
Cuando pide deshacer los últimos 20 minutos sin confirmarlo de forma interactiva
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

## Requisitos Técnicos

- Conjunto = operaciones del worktree con momento ≥ ahora − duración; el destino es el estado previo a la más antigua (ADR-TMC-003 § 4).
- Duraciones con formato `20m`, `2h` y acotadas a la retención; inválidas, rechazo sin cambios (SEC-TMC-07, BR-TMC-VAL-001).
- Permisos de US-TMC-013 sobre cada operación del conjunto y solape de US-TMC-012.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-010](../dev-specs/US-TMC-010-undo-since.md)

## Dependencias

- **Historias**: US-TMC-002; US-TMC-012 (solape) y US-TMC-013 (permisos) si el periodo incluye operaciones de otro actor.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.

## Estado de la implementación (2026-11-04)

Implementado en: PR #PR_NUM.

- **Hecho:** `raptor undo --since <duración>` (CLI) y `timemachine.undo` con `since` en el canal del daemon. Deshace, como un solo undo con su previo, el sufijo de la pila del worktree cuyas operaciones ocurrieron en el periodo, de cualquier actor. El destino es el previo de la más antigua. Si una operación del periodo no se puede deshacer, se rechaza la petición entera (D-TMC-29). El trabajo de otro actor pide la confirmación de US-TMC-013, y el reto muestra cuántas operaciones deshace. El solape de US-TMC-012 cuenta lo que cambió cada operación del conjunto. `UndoResult.undone_operation_ids` y `McpUndoResult.undone_count` dicen qué se deshizo. `agent` y `operation_id` siguen en `NOT_IMPLEMENTED`.
- **Tests:** los seis escenarios en `crates/core/tests/us_tmc_010.rs` (daemon y Git reales, repos y perfiles temporales); los dos e2e del binario en `apps/cli/tests/us_tmc_010_process.rs` (en/es); unitarios en `undo_since_tests.rs`, `stack_done_tests.rs`, `timemachine_since_tests.rs` y `apps/cli/src/undo/since_tests.rs`; los de la revisión en `crates/core/tests/us_tmc_010_review.rs`, `undo_since_review_tests.rs` y `since_review_tests.rs`.
- **Decisiones de la implementación:** ver [DS-US-TMC-010, Enmienda 2026-11-04](../dev-specs/US-TMC-010-undo-since.md).
- **Pendiente:**
  - Linux y Windows (XP-67, etapa de validación multiplataforma).
  - El éxito de `--since` y `--json` por el binario no tiene e2e.
  - Revisión de seguridad M-03: una operación nueva de un dueño del conjunto, hecha fuera de lo confirmado, cuenta como propia en la fase 2 del solape.
  - L-03/L-04: coste de un periodo largo bajo el mutex del oplog y en el solape.
  - Sin certificar: la fase P3 de `/implement` corta la suite a los 15 min.
