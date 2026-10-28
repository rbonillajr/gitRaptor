---
id: US-TMC-024
title: "Una restauración nunca retira trabajo de otro actor sin que el desarrollador lo vea y lo confirme"
type: us
status: draft
priority: high
created: 2026-10-27
updated: 2026-10-28
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-009, US-TMC-012, US-TMC-013]
covers: [BR-TMC-WF-003, BR-TMC-AUTH-001, BR-TMC-CONS-005, D-TMC-13, D-TMC-20, D-TMC-23, D-TMC-27, D-TMC-30]
blocked_by: []
tags: [time-machine, restauracion, solape]
---

# US-TMC-024: Una restauración nunca retira trabajo de otro actor sin que el desarrollador lo vea y lo confirme

## Descripción

**Como** desarrollador orquestador
**Quiero** que, antes de restaurar un punto, se me muestre qué trabajo de otro actor se perderá, que solo se retire si lo confirmo y que la restauración se detenga ante cualquier cambio que no estaba en lo que confirmé
**Para** volver a un estado bueno conocido sin borrar a ciegas el trabajo que un agente hizo después de ese punto

**Valor**: hoy una restauración puede sobrescribir trabajo posterior de otro actor que un undo habría detenido (TQ-18). Esta historia cierra ese hueco de NFR-01 y bloquea el cierre de M2.

## Reglas cubiertas

BR-TMC-WF-003 (restauración: plan, confirmación y cambios fuera del plan, D-TMC-30) · BR-TMC-AUTH-001 (solicitante; confirmación en macOS y Linux; rechazo para un agente, en Windows y por MCP) · BR-TMC-CONS-005 (sin continuar tras detenerse, D-TMC-27; ediciones sin Git como "sin agente") · D-TMC-13, D-TMC-20, D-TMC-23 — ver [business-rules.md](../business-rules.md)

**Decisión del orquestador (2026-10-27), validada por PO y Arquitecto (D-TMC-30)**: la restauración retira, con la confirmación interactiva de BR-TMC-AUTH-001, el trabajo de otro actor que lista su plan, y se detiene ante lo que no está en él. Una edición sin commitear en un worktree del conjunto distinto de aquel donde se pide cuenta como trabajo de otro actor ("sin agente"). Un solicitante agente nunca confirma: si el plan incluye trabajo de otro actor, la restauración se rechaza.

## Criterios de Aceptación

**Escenario: Restaurar sobre trabajo de otro actor muestra lo que se perderá y procede con confirmación**

Dado que después del punto de las 10:00 "claude-1" hizo un commit en "feat-login"
  Y el solicitante de la restauración queda sin atribuir y pide desde macOS o Linux
Cuando pide restaurar "feat-login" al punto de las 10:00
Entonces antes de cambiar nada recibe el commit de "claude-1" como trabajo de otro actor que se perderá, con su actor y su momento
Cuando lo confirma de forma interactiva
Entonces "feat-login" queda como estaba a las 10:00
  Y el commit de "claude-1" se recupera deshaciendo la restauración

**Escenario: Sin confirmación, la restauración no cambia nada**

Dado que después del punto de las 10:00 "claude-1" hizo un commit en "feat-login"
  Y un solicitante sin atribuir pide desde macOS o Linux restaurar "feat-login" al punto de las 10:00
Cuando no confirma la retirada del trabajo de "claude-1"
Entonces la restauración no se ejecuta
  Y el repo no cambia

**Escenario: Las ediciones sin commitear de otro worktree del conjunto cuentan como trabajo de otro actor**

Dado que la restauración de "feat-login" al punto de las 10:00 alcanza también el worktree "feat-login-v2"
  Y "feat-login-v2" tiene ediciones sin commitear en "api.rs" posteriores al punto
Cuando un solicitante sin atribuir pide desde macOS o Linux restaurar "feat-login" al punto de las 10:00
Entonces antes de cambiar nada recibe las ediciones de "api.rs" en "feat-login-v2" como trabajo "sin agente" que se perderá
  Y sin su confirmación interactiva ningún worktree cambia

**Escenario: Un cambio que no estaba en lo confirmado detiene la restauración**

Dado que un solicitante sin atribuir confirmó restaurar "feat-login" al punto de las 10:00 con el commit de "claude-1" como único trabajo de otro actor
  Y antes de que la restauración se aplique "claude-1" hace un commit que cambia "api.rs" en "feat-login"
Cuando la restauración se va a aplicar
Entonces la restauración se detiene sin cambiar el repo
  Y el solicitante recibe el cambio nuevo con su actor y el momento en que se vio
  Y no se le ofrece continuar a pesar del cambio

**Escenario: Si no se puede comprobar que no apareció trabajo nuevo, la restauración se detiene**

Dado que un solicitante sin atribuir confirmó restaurar "feat-login" al punto de las 10:00
  Y entre la confirmación y la aplicación el observador perdió eventos de "feat-login" (por ejemplo, por un desbordamiento de la vigilancia de archivos) sin que el motor se reiniciara
Cuando la restauración se va a aplicar
Entonces la restauración se detiene con el motivo de que no se pudo comprobar el trabajo posterior
  Y el repo no cambia

**Esquema del escenario: Un agente no puede retirar trabajo de otro actor al restaurar**

Dado que después del punto de las 10:00 hubo <trabajo posterior>
Cuando "<agente>" pide restaurar "feat-login" al punto de las 10:00
Entonces la restauración se rechaza con el motivo, sin pedir confirmación
  Y el repo no cambia

Ejemplos:

| agente | trabajo posterior |
|--------|-------------------|
| claude-2 | un commit de "claude-1" en "feat-login" |
| claude-1 | ediciones sin commitear en "api.rs" del worktree "feat-login-v2", alcanzado por la restauración |
| claude-1 | ediciones sin commitear en "api.rs" de "feat-login", aunque las haya hecho el propio "claude-1" |

Un agente nunca puede restaurar un worktree con ediciones sin commitear, ni siquiera las suyas: en el MVP cuentan como "sin agente" (D-TMC-27, D-TMC-30). Si el motor se reinicia entre la confirmación y la aplicación, la confirmación caduca: el repo tampoco cambia, pero el motivo es la confirmación caducada, no el hueco.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-009 (restaurar a un punto), US-TMC-012 (detección del solape) y US-TMC-013 (solicitante y confirmación interactiva).
- **Externas**: ninguna. Bloquea el cierre de M2 (fila "no pisar a otro" de [release-plan](../../../release-plan.md)).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux salvo la confirmación interactiva, que en el MVP no existe en Windows ni por MCP (D-TMC-23); mensajes en inglés y español.
