---
id: US-WTL-011
title: "El desarrollador sabe en qué va el CI y el PR de la rama de cada agente sin salir del cockpit"
type: us
status: draft
priority: low
milestone: post-v0.1.0
blocked_by:
  - US-WTL-003
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  business:
    - BRD-GRP-001
  stories:
    - US-WTL-003
  rules:
    - BR-WTL-009
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - ci
  - pr
  - could
---

# US-WTL-011: El desarrollador sabe en qué va el CI y el PR de la rama de cada agente sin salir del cockpit

## Descripción

**Como** desarrollador orquestador, **quiero** ver junto a cada rama de agente el resultado de su CI y el estado de su PR, **para** decidir qué revisar o integrar sin abrir el navegador por cada agente.

**Valor**: BR-17 y BR-19 (Should en el BRD; Could en esta feature, después de v0.1.0).

## Criterios de Aceptación

**Escenario: Resumen por rama**

Dado "feat-a" con su CI en verde y un PR abierto
Cuando el desarrollador consulta la flota
Entonces "feat-a" muestra el CI en verde y el PR abierto

**Escenario: Rama sin PR**

Dado "feat-b" sin PR
Cuando el desarrollador consulta la flota
Entonces "feat-b" indica que no tiene PR

**Escenario: El proveedor no responde**

Dado el proveedor del CI o del PR sin respuesta
Cuando el desarrollador consulta la flota
Entonces cada rama indica que el resumen no está disponible y el resto de la flota se muestra igual

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño.

## Dependencias

- **De User Stories**: Requiere US-WTL-003 (lectura del CI de GitHub Actions en M3).
- **Pendiente para pasar a ready**: qué proveedores entran (GitHub, Azure DevOps por BR-19) y qué datos del PR se muestran; sin decidir en CTX-WTL-001.
