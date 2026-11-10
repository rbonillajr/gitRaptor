---
id: US-WTL-012
title: "El desarrollador levanta a la vez el servidor de varios agentes sin que choquen sus puertos"
type: us
status: draft
priority: low
milestone: post-v0.1.0
blocked_by:
  - US-WTL-001
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  stories:
    - US-WTL-001
  rules: []
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - puerto
  - could
---

# US-WTL-012: El desarrollador levanta a la vez el servidor de varios agentes sin que choquen sus puertos

## Descripción

**Como** desarrollador orquestador, **quiero** que cada worktree tenga su propio puerto asignado, **para** probar en paralelo lo que hizo cada agente sin pelear por el mismo puerto.

**Valor**: BR-07 (Could, después de v0.1.0).

## Criterios de Aceptación

**Escenario: Puertos distintos por worktree**

Dado los worktrees "feat-a" y "feat-b" creados por GitRaptor
Cuando el desarrollador consulta el puerto de cada uno
Entonces cada uno tiene un puerto distinto y estable mientras exista

**Escenario: Puerto ocupado**

Dado el puerto asignado a "feat-a" ocupado por otro programa
Cuando el desarrollador lo consulta
Entonces GitRaptor indica que el puerto está ocupado

**Escenario: Puerto liberado al descartar**

Dado "feat-a" descartado
Cuando el desarrollador crea "feat-c"
Entonces "feat-c" puede recibir el puerto que tenía "feat-a"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño.

## Dependencias

- **De User Stories**: Requiere US-WTL-001.
- **Pendiente para pasar a ready**: CTX-WTL-001 no tiene regla para el puerto por worktree; rango, cómo llega el puerto al proyecto y si se reasigna son `sin dato`. Hace falta una regla de negocio antes de refinarla.
