---
id: US-DSK-012
title: "El usuario crea, cambia y borra ramas desde la app sin perder trabajo"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-GRD-008
    - US-GRD-017
    - US-TMC-001
    - US-DSK-005
    - US-DSK-006
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - ramas
  - must
---

# US-DSK-012: El usuario crea, cambia y borra ramas desde la app sin perder trabajo

## Descripción

**Como** desarrollador novato, **quiero** ver todas las ramas del repo y crear, cambiar o borrar una con un clic, **para** organizar mi trabajo sin miedo a perder lo que no he commiteado.

**Valor**: BR-27 (UI-2, Must). Nivel **UI-2**. Imprescindible.

## Reglas

- Cambiar de rama con trabajo sin commitear nunca lo pierde: hay punto de recuperación previo (US-TMC-001, NFR-01).
- Borrar una rama es destructivo: exige punto previo (US-GRD-017) y respeta las ramas protegidas (US-GRD-008).
- Una rama sacada en otra copia de trabajo no se puede sacar ni borrar aquí; el usuario sabe en cuál está.

## Dependencias

- **Reutiliza**: la operación `create-worktree` del catálogo para la variante "rama nueva para un agente" (US-DSK-016); US-GRD-008, US-GRD-017, US-TMC-001.
- **Historias**: US-DSK-005 (el grafo, que en UI-2 muestra todas las ramas), US-DSK-006 (deshacer).
- **Qué falta en el motor**: DEP-DSK-3 (lista de todas las ramas y sus commits, no solo las de los agentes) y DEP-DSK-4 (crear una rama suelta, cambiar de rama y borrar una rama). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Crear una rama y cambiarse a ella**

Dado "main" sacada en la copia de trabajo principal
Cuando el usuario crea "fix-login" desde "main" y se cambia a ella
Entonces la copia de trabajo principal tiene "fix-login" sacada

**Escenario: Cambiar de rama con trabajo sin commitear**

Dado "fix-login" con "login.rs" cambiado sin commitear
Cuando el usuario se cambia a "main"
Entonces el cambio de "login.rs" queda guardado en un punto de recuperación
  Y el usuario sabe cómo recuperarlo

**Escenario: Borrar una rama protegida**

Dado "main" como rama protegida
Cuando el usuario intenta borrar "main"
Entonces "main" no se borra y el usuario conoce la regla que lo impide

**Escenario: Rama sacada en otra copia de trabajo**

Dado "feat-pagos" sacada en la copia de trabajo de "claude-1"
Cuando el usuario intenta cambiarse a "feat-pagos" en la copia principal
Entonces la copia principal no cambia
  Y el usuario sabe que "feat-pagos" está en la copia de trabajo de "claude-1"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
