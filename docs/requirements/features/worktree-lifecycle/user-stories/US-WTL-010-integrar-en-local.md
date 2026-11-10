---
id: US-WTL-010
title: "El desarrollador integra en local el trabajo de un agente y cierra su worktree en un paso que puede deshacer"
type: us
status: draft
priority: low
milestone: post-v0.1.0
blocked_by:
  - US-CKP-014
  - US-WTL-002
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  stories:
    - US-CKP-006
    - US-CKP-014
    - US-WTL-002
  rules:
    - BR-WTL-006
    - BR-WTL-007
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - land
  - could
---

# US-WTL-010: El desarrollador integra en local el trabajo de un agente y cierra su worktree en un paso que puede deshacer

## Descripción

**Como** desarrollador orquestador, **quiero** integrar en local la rama de un agente terminado y descartar su worktree en un solo paso, con la predicción de conflictos, las políticas del repo y un snapshot, **para** cerrar el trabajo de un agente tan rápido como con `wt merge` sin perder la red de seguridad.

**Valor**: BR-07 (Could, después de v0.1.0).

## Criterios de Aceptación

**Escenario: Integrar y cerrar**

Dado "feat-a" de "claude-1", terminado, sin conflictos previstos con `main`
Cuando el desarrollador integra "feat-a" en local
Entonces GitRaptor guarda antes un snapshot, integra "feat-a" en `main` y descarta su worktree y su rama
  Y todo se deshace en un paso

**Escenario: Conflicto previsto**

Dado "feat-b" con un conflicto previsto con `main`
Cuando el desarrollador pide integrarlo en local
Entonces GitRaptor informa el conflicto previsto antes de tocar nada

**Escenario: La política lo impide**

Dado una política del repo que marca `main` como rama protegida
Cuando el desarrollador pide integrar "feat-c"
Entonces la operación se bloquea con el motivo de la política y no se toca nada

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño.

## Dependencias

- **De User Stories**: Requiere US-CKP-014 (integrar con Deshacer) y US-WTL-002 (descarte reversible) · Relacionado US-CKP-006 (predicción).
- **Pendiente para pasar a ready**: qué estrategias de integración ofrece (squash, rebase, fast-forward) y si descarta el worktree siempre o a elección; sin decidir en CTX-WTL-001.
