---
id: US-WTL-007
title: "El desarrollador prueba en su copia el trabajo de un agente y se lo devuelve sin perder nada de ninguno de los dos"
type: us
status: ready
priority: medium
milestone: M4
blocked_by:
  - US-WTL-001
created: 2026-11-10
updated: 2026-11-21
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-TMC-001
  stories:
    - US-WTL-001
    - US-TMC-002
  rules:
    - BR-WTL-013
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - handoff
  - time-machine
  - should
---

# US-WTL-007: El desarrollador prueba en su copia el trabajo de un agente y se lo devuelve sin perder nada de ninguno de los dos

## Descripción

**Como** desarrollador orquestador, **quiero** «Traer a mi copia» el trabajo de un agente y luego «Devolver» el resultado a su worktree, con snapshot de los dos lados, **para** probarlo en mi editor y mi entorno sin miedo a pisar mi trabajo ni el del agente.

**Valor**: BR-07 (Should, v0.1.0); NFR-01. En la CLI y en la app.

## Criterios de Aceptación

**Escenario: Traer a mi copia**

Dado "feat-a" de "claude-1", terminado, con 2 commits y 1 archivo sin commitear
  Y la copia del desarrollador con 1 archivo sin commitear propio
Cuando el desarrollador trae "feat-a" a su copia
Entonces GitRaptor guarda antes un snapshot de la copia del desarrollador y de "feat-a"
  Y la copia del desarrollador queda con el trabajo de "feat-a"

**Escenario: Devolver**

Dado el trabajo de "feat-a" traído a la copia del desarrollador con 1 cambio nuevo suyo
Cuando el desarrollador lo devuelve
Entonces GitRaptor guarda antes un snapshot de los dos lados
  Y "feat-a" queda con el trabajo y el cambio nuevo

**Escenario: Deshacer el handoff**

Dado un «Traer a mi copia» recién hecho
Cuando el desarrollador lo deshace
Entonces su copia y "feat-a" vuelven exactamente al estado de antes

**Escenario: El snapshot falla**

Dado "feat-a" terminado
Cuando el desarrollador pide traerlo y el snapshot de cualquiera de los dos lados falla
Entonces no se mueve nada y GitRaptor informa que no hubo snapshot

**Escenario: Con el agente vivo no se trae ni se devuelve**

Dado "feat-a" de "claude-1", con "claude-1" todavía activo
Cuando el desarrollador pide traer "feat-a" a su copia o devolvérselo
Entonces la petición se rechaza indicando que "claude-1" sigue vivo y que hay que terminarlo antes
  Y ni la copia del desarrollador ni "feat-a" cambian

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (acciones «Traer a mi copia» y «Devolver» en la CLI y la app).

## Dependencias

- **De User Stories**: Requiere US-WTL-001 · Relacionado US-TMC-002 (Deshacer).
- **Agente vivo (Pregunta 18 de CTX-WTL-001, decidida el 2026-11-10)**: aplica la misma protección que al descartar; con el agente vivo, traer y devolver se rechazan hasta terminarlo. Lo cubre el escenario «Con el agente vivo no se trae ni se devuelve».
- **Cross-cutting / transversales**: qué hace el handoff cuando los dos lados chocan: transversal (lo define el Arquitecto con el flujo de operación detenida de US-CKP-016).
