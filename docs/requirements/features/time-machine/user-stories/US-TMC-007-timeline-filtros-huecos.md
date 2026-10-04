---
id: US-TMC-007
title: "El desarrollador filtra el timeline por worktree, agente o periodo y ve lo que no se observó"
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
  stories: [US-TMC-006, US-GRP-005]
covers: [BR-TMC-EDGE-002, D-TMC-7, D-TMC-19]
blocked_by: []
tags: [time-machine, timeline, huecos]
---

# US-TMC-007: El desarrollador filtra el timeline por worktree, agente o periodo y ve lo que no se observó

## Descripción

**Como** desarrollador orquestador
**Quiero** filtrar el timeline por worktree, por agente y por periodo, y ver los huecos de observación
**Para** encontrar en segundos lo que hizo un agente y no fiarme de lo que nadie observó

**Valor**: con 3 a 10 agentes, el timeline completo es ruido; los filtros lo vuelven útil y los huecos evitan falsas certezas.

## Reglas cubiertas

BR-TMC-EDGE-002 (huecos explícitos) · D-TMC-7, D-TMC-19 (timeline por repo con filtros) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Filtrar por worktree**

Dado actividad en "feat-login" y en "feat-pagos"
Cuando el desarrollador filtra el timeline por "feat-login"
Entonces el resultado contiene solo eventos de "feat-login"

**Escenario: Filtrar por agente y periodo**

Dado eventos de "claude-1" y de "claude-2" durante la última hora
Cuando el desarrollador filtra por "claude-1" y los últimos 20 minutos
Entonces el resultado contiene solo eventos de "claude-1" ocurridos en esos 20 minutos

**Escenario: Un hueco de observación queda explícito**

Dado que el repo no se observó entre las 12:00 y las 13:00 y en ese periodo aparecieron dos commits
Cuando el desarrollador consulta el timeline
Entonces el timeline indica el hueco de 12:00 a 13:00
  Y los dos commits figuran como "Tú u otro (sin atribuir)"
  Y el hueco no ofrece ningún punto para restaurar

**Escenario: Filtro por un agente sin actividad**

Dado que "claude-9" no tiene eventos en el repo
Cuando el desarrollador filtra el timeline por "claude-9"
Entonces el resultado está vacío
  Y el desarrollador recibe el aviso de que ese agente no tiene actividad

**Escenario: Periodo no válido**

Dado un timeline con actividad
Cuando el desarrollador filtra por el periodo "veinte"
Entonces el filtro se rechaza con el motivo
  Y no se aplica ningún filtro

## Requisitos Técnicos

- Filtros por worktree, agente vigente y periodo resueltos en la consulta del oplog; los huecos vienen del motor (ADR-GRP-013 § 5) y se intercalan como intervalos.
- Dentro de un hueco no se ofrecen puntos de restauración sin snapshot (BR-TMC-EDGE-002).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-006; US-GRP-005 (huecos "sin atribuir") de motor-local.
- **Externas**: presentación en la TUI con F-001-02 Cockpit.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
