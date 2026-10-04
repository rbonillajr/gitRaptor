---
id: US-TMC-020
title: "El desarrollador y sus agentes no notan el coste de los snapshots"
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
  stories: [US-TMC-001, US-TMC-004]
covers: [D-TMC-21]
blocked_by: ["spike (a): definición de repo mediano (D-TMC-21)"]
tags: [time-machine, rendimiento, nfr-04]
---

# US-TMC-020: El desarrollador y sus agentes no notan el coste de los snapshots

## Descripción

**Como** desarrollador orquestador
**Quiero** que guardar un snapshot añada menos de 200 ms a cada operación en un repo mediano
**Para** no tener motivos para desactivar la red de seguridad (NFR-04)

**Valor**: un snapshot lento frena a los agentes y el usuario acaba apagándolo (R1).

## Reglas cubiertas

NFR-04 · D-TMC-21 ("repo mediano" se fija en el spike a) — ver [context.md](../context.md)

## Criterios de Aceptación

**Escenario: Overhead dentro del objetivo**

Dado un repo mediano de referencia con trabajo sin commitear en un worktree
Cuando se lanza desde GitRaptor una operación que lo modifica
Entonces el snapshot previo añade menos de 200 ms a la operación

**Escenario: Con 10 worktrees activos**

Dado el repo de referencia con 10 worktrees activos con cambios
Cuando se lanza desde GitRaptor una operación en uno de ellos
Entonces el snapshot previo sigue añadiendo menos de 200 ms

**Escenario: Un repo fuera de la referencia no deja de estar protegido**

Dado un repo mayor que el de referencia
Cuando se lanza desde GitRaptor una operación que lo modifica
Entonces la operación sigue precedida de su snapshot previo aunque tarde más de 200 ms

## Requisitos Técnicos

- Presupuesto y medición de ADR-TMC-006: p95 < 200 ms del snapshot previo, con almacén sembrado, en el repo de referencia que fija SPIKE-TMC-001.
- Ampliar el banco de INF-GRP-002 con el escenario de operación protegida con trabajo sin commitear, con 1 y 10 worktrees; gate de CI y aviso por etapa.
- Bloqueada hasta el cierre de SPIKE-TMC-001 (D-TMC-21).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-004.
- **Externas**: **bloqueada** hasta que el spike (a) fije el "repo mediano" de referencia (D-TMC-21, Arquitecto).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
