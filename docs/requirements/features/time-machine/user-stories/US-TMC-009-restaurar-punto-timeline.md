---
id: US-TMC-009
title: "El desarrollador devuelve su worktree a cualquier punto del timeline"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-001, US-TMC-002, US-TMC-006, US-TMC-013]
covers: [BR-TMC-WF-003, BR-TMC-CONS-001, BR-TMC-EDGE-002, BR-TMC-AUTH-001, D-TMC-20, D-TMC-23]
blocked_by: []
tags: [time-machine, restauracion]
---

# US-TMC-009: El desarrollador devuelve su worktree a cualquier punto del timeline

## Descripción

**Como** desarrollador orquestador
**Quiero** restaurar mi worktree a un punto concreto del timeline
**Para** volver a un estado bueno conocido aunque después hubo varias operaciones

**Valor**: recupera de una cadena de errores en un paso, y la restauración se puede deshacer.

## Reglas cubiertas

BR-TMC-WF-003 (restaurar; alcance D-TMC-20) · BR-TMC-CONS-001 (punto previo a la restauración) · BR-TMC-EDGE-002 (sin puntos en huecos) · BR-TMC-AUTH-001 (trabajo de otro actor, D-TMC-23) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Restaurar el worktree a un punto anterior**

Dado que "feat-login" tuvo tres operaciones de un solicitante que queda sin atribuir después del punto de las 10:00
Cuando ese solicitante restaura "feat-login" al punto de las 10:00
Entonces "feat-login" queda exactamente como estaba a las 10:00, incluido su trabajo sin commitear

**Escenario: La restauración alcanza lo que cambió después del punto**

Dado que después del punto de las 10:00 un solicitante sin atribuir borró la rama "feat-login-v2" creada desde "feat-login"
Cuando ese solicitante restaura "feat-login" al punto de las 10:00
Entonces la rama "feat-login-v2" vuelve a existir
  Y los worktrees que no cambiaron después de ese punto no se modifican

**Escenario: La restauración se puede deshacer**

Dado una restauración de "feat-login" al punto de las 10:00
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces "feat-login" vuelve al estado previo a la restauración

**Escenario: Un punto incompleto o dentro de un hueco no se restaura**

Dado un punto que no tiene snapshot completo
Cuando un solicitante sin atribuir intenta restaurar a ese punto
Entonces la restauración se rechaza con el motivo
  Y el repo no cambia

**Escenario: Restaurar sobre trabajo de otro actor exige confirmación**

Dado que después del punto de las 10:00 "claude-1" hizo un commit en "feat-login"
  Y el solicitante de la restauración queda sin atribuir
Cuando pide restaurar "feat-login" al punto de las 10:00
  Y lo confirma de forma interactiva
Entonces "feat-login" queda como estaba a las 10:00, sin el commit de "claude-1"

**Escenario: Otro agente no puede restaurar sobre trabajo ajeno**

Dado que después del punto de las 10:00 "claude-1" hizo un commit en "feat-login"
Cuando "claude-2" pide restaurar "feat-login" al punto de las 10:00
Entonces la restauración se rechaza con el motivo
  Y el repo no cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-002, US-TMC-006, US-TMC-013.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
