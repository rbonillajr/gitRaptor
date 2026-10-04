---
id: US-TMC-010
title: "El desarrollador deshace todo lo ocurrido en su worktree en los últimos minutos"
type: us
status: draft
priority: medium
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
  stories: [US-TMC-002, US-TMC-012, US-TMC-013]
covers: [BR-TMC-WF-001, BR-TMC-VAL-001, BR-TMC-AUTH-001, D-TMC-19, D-TMC-23]
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

BR-TMC-WF-001 (ámbito: el worktree desde el que se invoca) · BR-TMC-VAL-001 (periodo inválido o vacío) · BR-TMC-AUTH-001 (operaciones de otro actor, D-TMC-23) — ver [business-rules.md](../business-rules.md)

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

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002; US-TMC-012 (solape) y US-TMC-013 (permisos) si el periodo incluye operaciones de otro actor.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
