---
id: US-DSK-016
title: "El usuario prepara una copia de trabajo para un agente nuevo y descarta la de uno que terminó"
type: us
status: draft
priority: medium
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-CKP-017
    - US-CKP-018
    - US-CKP-020
    - US-DSK-004
    - US-DSK-006
  specs:
    - DS-US-CKP-018
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - worktrees
  - should
---

# US-DSK-016: El usuario prepara una copia de trabajo para un agente nuevo y descarta la de uno que terminó

## Descripción

**Como** usuario que reparte trabajo entre agentes, **quiero** crear en un paso una copia de trabajo con su rama para un agente nuevo y descartar la de un agente cuyo trabajo ya no sirve, **para** poner a trabajar a otro agente sin pisar a los demás y limpiar sin miedo a perder nada.

**Valor**: BR-27 (UI-2, Should); BR-07. Nivel **UI-2**. Imprescindible. Patrón P5 del [benchmark](../research/benchmark-2026-10.md): el usuario no necesita saber qué es un worktree.

## Reglas

- Las del Cockpit: crear desde la base confirmada (US-CKP-018) y descartar con punto previo y recuperable (US-CKP-017).
- Descartar el trabajo de otro actor exige confirmar el plan concreto (US-CKP-020; Q-DSK-3).
- La app no lanza al agente (BRD § 6.4): deja la copia lista y dice dónde está.

## Dependencias

- **Reutiliza**: US-CKP-017, US-CKP-018 (DS-US-CKP-018), US-CKP-020; operaciones `create-worktree` y `discard-worktree`.
- **Historias**: US-DSK-004 (aparece en la flota), US-DSK-006 (recuperar lo descartado).
- **Qué falta en el motor**: DEP-DSK-1 para descartar trabajo ajeno. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Preparar una copia para un agente nuevo**

Dado "tienda" con "main" como base confirmada
Cuando el usuario prepara una copia de trabajo llamada "feat-envios"
Entonces existe una copia de trabajo con la rama "feat-envios" creada desde "main"
  Y aparece en la flota sin agente, lista para usar

**Escenario: Descartar y recuperar**

Dado "feat-viejo" de "claude-2", terminada, con 3 commits que no se integraron
Cuando el usuario confirma descartarla
Entonces la copia de trabajo y la rama "feat-viejo" dejan de existir
  Y el usuario puede recuperarlas desde el historial

**Escenario: El nombre ya existe**

Dado una rama "feat-envios" que ya existe
Cuando el usuario intenta preparar otra copia de trabajo con ese nombre
Entonces no se crea nada y el usuario sabe que el nombre está en uso

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
