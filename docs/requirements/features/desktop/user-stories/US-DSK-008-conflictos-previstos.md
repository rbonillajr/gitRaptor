---
id: US-DSK-008
title: "El usuario ve qué agentes van a chocar antes de que choquen y en qué archivos"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-CKP-006
    - US-CKP-007
    - US-CKP-008
    - US-CKP-009
    - US-DSK-004
    - US-DSK-005
  enablers:
    - TS-CKP-001
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - prediccion-conflictos
  - must
---

# US-DSK-008: El usuario ve qué agentes van a chocar antes de que choquen y en qué archivos

## Descripción

**Como** usuario con varios agentes en paralelo, **quiero** que la app me avise cuando dos agentes, o un agente y la base, van a chocar, y en qué archivos, **para** reorganizar el trabajo antes del merge en vez de descubrir el conflicto al final.

**Valor**: BR-27 (UI-1, Must); BR-06. Nivel **UI-1**. Es el primer diferencial del [benchmark](../research/benchmark-2026-10.md) § 4.

## Reglas

- Las de la predicción del Cockpit: dos niveles, solape (incluye lo sin commitear) y conflicto previsto (merge en seco de lo commiteado), según BR-CKP-CALC-002 y Q-CKP-5.
- Una predicción vieja nunca se presenta como actual: siempre se ve su antigüedad (US-CKP-007, BR-CKP-CALC-003).
- Con la rama base sin confirmar, la predicción contra la base queda pendiente (US-CKP-009).
- La predicción nunca escribe en el repo.

## Dependencias

- **Reutiliza**: US-CKP-006, US-CKP-007, US-CKP-008, US-CKP-009; TS-CKP-001 (predictor en el daemon, en parte) y SPIKE-CKP-001; `prediction.list`, `prediction.pairs`, `prediction.changed`.
- **Historias**: US-DSK-004 (tarjetas de la flota) y US-DSK-005 (carriles), donde se señala el choque.
- **Qué falta en el motor**: lo que le falte a TS-CKP-001 (el trabajador, el pool y la publicación); nada propio de la app.

## Criterios de Aceptación

**Escenario: Dos agentes van a chocar**

Dado "claude-1" y "claude-2" cambiando las mismas líneas de "pagos.rs" en "feat-pagos" y "feat-login" con commits
Cuando el motor publica la predicción
Entonces el usuario ve un conflicto previsto entre "feat-pagos" y "feat-login" con "pagos.rs" como archivo en conflicto

**Escenario: Aviso de un conflicto nuevo**

Dado la app abierta en "tienda" sin conflictos previstos
Cuando aparece un conflicto previsto nuevo
Entonces el usuario recibe un aviso dentro de la app con el par de agentes afectado

**Escenario: La predicción se está recalculando**

Dado que la rama base de "tienda" acaba de moverse
Cuando el usuario consulta los conflictos previstos
Entonces sabe que se están recalculando y ve la antigüedad del último resultado

**Escenario: Base sin confirmar**

Dado "tienda" con la rama base pendiente de confirmar
Cuando el usuario consulta los conflictos contra la base
Entonces sabe que esa predicción está pendiente hasta que se confirme la base

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), conflictos previstos.
- **Dev Spec:** pendiente.
