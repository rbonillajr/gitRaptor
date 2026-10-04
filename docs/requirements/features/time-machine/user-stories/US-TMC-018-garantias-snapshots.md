---
id: US-TMC-018
title: "Los snapshots no se publican, no se pierden con el mantenimiento de Git y ningún agente los altera"
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
  stories: [US-TMC-001]
covers: [BR-TMC-CONS-004, D-TMC-3, D-TMC-11]
blocked_by: []
tags: [time-machine, snapshots, garantias]
---

# US-TMC-018: Los snapshots no se publican, no se pierden con el mantenimiento de Git y ningún agente los altera

## Descripción

**Como** desarrollador orquestador
**Quiero** que los snapshots no viajen al remoto, no desaparezcan con el mantenimiento de Git y no cambien cuando un agente trabaja
**Para** contar con ellos el día que los necesite y no filtrar trabajo sin commitear al equipo

**Valor**: un snapshot que se puede perder o publicar no es una red de seguridad.

## Reglas cubiertas

BR-TMC-CONS-004 (escrituras propias, explícitas y recuperables; tres garantías) · D-TMC-3, D-TMC-11 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Un push no publica snapshots**

Dado un repo con snapshots guardados
Cuando se empujan todas las ramas al remoto
Entonces el remoto no recibe ningún snapshot

**Escenario: El mantenimiento de Git no borra snapshots**

Dado un repo con snapshots dentro de la retención
Cuando se ejecuta la limpieza de mantenimiento de Git con la máxima agresividad
Entonces todos esos snapshots siguen disponibles para restaurar

**Escenario: El trabajo de un agente no altera los snapshots**

Dado un snapshot guardado de "feat-login"
Cuando un agente hace checkout, reset, limpieza de archivos sin seguimiento y commits en "feat-login"
Entonces el contenido de ese snapshot no cambia

**Escenario: Guardar un snapshot no cambia el repo del usuario**

Dado un repo observado
Cuando la Time Machine guarda un snapshot
Entonces el estado de los worktrees, las ramas visibles y los cambios pendientes del usuario no cambian

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001.
- **Externas**: ninguna. El mecanismo y la ubicación los decide el Arquitecto con estas garantías (D-TMC-11).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
