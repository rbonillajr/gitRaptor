---
id: US-DSK-006
title: "El usuario deshace con un botón lo que hizo un agente o él mismo, y puede rehacerlo"
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
    - US-CKP-021
    - US-TMC-002
    - US-TMC-003
    - US-TMC-006
    - US-TMC-009
    - US-TMC-012
    - US-TMC-024
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - time-machine
  - must
---

# US-DSK-006: El usuario deshace con un botón lo que hizo un agente o él mismo, y puede rehacerlo

## Descripción

**Como** desarrollador novato, **quiero** ver en un historial qué cambió en mi repo, cuándo y quién lo hizo, y deshacer cualquier operación con un botón, **para** recuperar mi trabajo, también el que no había commiteado, sin saber Git ni usar el reflog.

**Valor**: BR-27 (UI-1, Must); BR-08, BR-09 y BR-10. Nivel **UI-1**. Patrones P1 y P2 del [benchmark](../research/benchmark-2026-10.md).

## Reglas

- Las de la Time Machine: un undo nunca sobrescribe trabajo posterior de otro actor (US-TMC-012) y una restauración nunca retira trabajo ajeno sin que el usuario lo vea y lo confirme (US-TMC-024).
- Deshacer el trabajo de un agente es tocar trabajo ajeno: exige la confirmación de ADR-TMC-005 § 3, nunca un control más débil (Q-DSK-3).
- Todo undo es reversible: deshacer crea una entrada nueva que se puede rehacer (US-TMC-003).
- Lo que se purgó o no se observó se dice; nunca se promete recuperar lo que no se capturó (aviso de purga de US-CKP-021, huecos de US-GRP-005).

## Dependencias

- **Reutiliza**: US-CKP-021 (historial y aviso de purga, DS-US-CKP-021), US-TMC-002, US-TMC-003, US-TMC-006, US-TMC-009, US-TMC-012, US-TMC-024; `timemachine.timeline`, `timemachine.undo`, `timemachine.redo`, `timemachine.restore`, `timemachine.notices`.
- **Qué falta en el motor**: DEP-DSK-1, la confirmación de trabajo ajeno desde la app (hoy exige terminal de control). Sin ella, en UI-1 el usuario deshace desde la app lo suyo y, para lo de un agente, recibe el comando exacto para la terminal. transversal (lo define el Arquitecto).
- **Plataforma**: en Windows no se confirma trabajo ajeno (BR-TMC-AUTH-001; R-DSK-8).

## Criterios de Aceptación

**Escenario: Deshacer lo que hizo un agente**

Dado que "claude-1" hizo "reset --hard" en "feat-pagos" y borró trabajo sin commitear
Cuando el usuario elige esa operación en el historial, revisa qué se recupera y confirma que es él quien deshace
Entonces "feat-pagos" vuelve al estado anterior al reset, con el trabajo sin commitear
  Y el historial registra el undo a nombre del usuario

**Escenario: Rehacer lo deshecho**

Dado que el usuario acaba de deshacer el reset de "feat-pagos"
Cuando decide rehacerlo
Entonces "feat-pagos" vuelve al estado posterior al reset

**Escenario: Trabajo posterior de otro actor**

Dado que "claude-2" cambió "pagos.rs" en "feat-pagos" después de la operación elegida
Cuando el usuario intenta deshacerla
Entonces el undo no sobrescribe el cambio de "claude-2"
  Y el usuario sabe qué archivos lo impiden

**Escenario: Punto ya purgado**

Dado una operación cuyo punto de recuperación ya se purgó
Cuando el usuario la consulta en el historial
Entonces sabe que ya no se puede deshacer y por qué

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), historial y deshacer.
- **Dev Spec:** pendiente.
