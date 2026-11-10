---
id: US-WTL-006
title: "El desarrollador crea worktrees con artefactos al día sin tener que reconstruir la semilla"
type: us
status: draft
priority: medium
milestone: M4
blocked_by:
  - US-WTL-001
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-GRP-015
  stories:
    - US-WTL-001
  rules:
    - BR-WTL-003
    - BR-WTL-012
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - semilla
  - daemon
  - should
---

# US-WTL-006: El desarrollador crea worktrees con artefactos al día sin tener que reconstruir la semilla

## Descripción

**Como** desarrollador orquestador, **quiero** que GitRaptor mantenga una semilla por toolchain y perfil de compilación y la reconstruya sola cuando la rama base avanza, **para** que cada worktree nuevo arranque con artefactos al día sin que yo tenga que acordarme de `raptor seed refresh` y sin quitarle recursos a los agentes.

**Valor**: BR-07 (Should, v0.1.0).

## Criterios de Aceptación

**Escenario: La base avanza y la semilla se reconstruye**

Dado una semilla de `main` del toolchain y perfil actuales
Cuando `main` avanza y llega un momento tranquilo
Entonces GitRaptor reconstruye la semilla sobre el nuevo `main`
  Y el siguiente `raptor new` copia los artefactos de la semilla reconstruida

**Escenario: Una semilla por toolchain y perfil**

Dado dos perfiles de compilación en uso en el repo
Cuando el desarrollador crea un worktree para cada perfil
Entonces cada uno copia desde la semilla de su toolchain y perfil

**Escenario: Nadie copia de una semilla en reconstrucción**

Dado la semilla en plena reconstrucción
Cuando el desarrollador ejecuta `raptor new feat-h --agent claude -- "tarea"`
Entonces ningún artefacto se copia desde la semilla mientras se reconstruye

**Escenario: La reconstrucción falla**

Dado una reconstrucción de la semilla que falla
Cuando termina con error
Entonces GitRaptor informa el fallo y no se copia nada desde una semilla incompleta

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño.

## Dependencias

- **De User Stories**: Requiere US-WTL-001 (semilla creada por `raptor new` y `raptor seed refresh`).
- **Pendiente para pasar a ready**: Pregunta 6 de CTX-WTL-001, qué es un «momento tranquilo» (`sin dato`, Rene y Arquitecto); y qué hace `raptor new` mientras la semilla se reconstruye (esperar o crear sin copia), sin decidir.
- **Cross-cutting / transversales**: consumo de recursos del daemon al reconstruir y dónde vive la semilla (Pregunta 9): transversal (lo define el Arquitecto).
