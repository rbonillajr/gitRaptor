---
id: US-CKP-024
title: "Integrar sigue siendo seguro cuando el estado cambia, choca o la base no está sacada"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-014
    - US-CKP-006
tags:
  - cockpit
  - acciones-por-agente
  - merge
  - operacion-protegida
  - must
---

# US-CKP-024: Integrar sigue siendo seguro cuando el estado cambia, choca o la base no está sacada

## Descripción

**Como** desarrollador orquestador, **quiero** que integrar me avise de un conflicto previsto, no ejecute dos veces ni sobre un estado que cambió, y funcione aunque ningún worktree tenga la base sacada, **para** confiar en la tecla de integrar en el caos de 10 agentes.

**Valor**: BR-07 (Must); NFR-01. Separada de US-CKP-014 para que el esqueleto de escritura quepa en una rama corta (ajuste del PO).

## Reglas cubiertas

BR-CKP-ELIG-002 (destino sin sesión presente, aviso por ⚡) · BR-CKP-ELIG-001 (columna merge) · BR-CKP-CONS-004 · BR-CKP-EDGE-009 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 (flujo de escritura); US-CKP-006 (⚡ publicado).
- **Técnicas**: TS-CKP-002 (revalidación bajo cerrojo y avance rápido sin worktree, ADR-CKP-002 § 5 y § 7).

## Criterios de Aceptación

**Escenario: Con ⚡ en el par, aviso y confirmación**

Dado ⚡ entre "feat-pagos" y "main"
Cuando el desarrollador pide integrar "feat-pagos"
Entonces el aviso reúne el conflicto previsto y el plan, y pide confirmación; si la rechaza, nada cambia

**Escenario: Destino con sesión presente**

Dado una sesión Inactiva de "claude-5" en el worktree principal, que tiene "main" sacada
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Integrar" aparece desactivado con "hay un agente en el worktree de destino"

**Escenario: Un commit entre preparar y ejecutar**

Dado el plan de integrar "feat-pagos" hasta "a1b2c3" confirmado
Cuando "claude-1" commitea "d4e5f6" antes de que se ejecute
Entonces la operación se rechaza con "el estado cambió" y "main" no cambia

**Escenario: Dos TUIs a la vez, una sola ejecución**

Dado dos TUIs abiertas en "shop"
Cuando las dos piden integrar "feat-pagos" al mismo tiempo
Entonces la primera se ejecuta y la segunda recibe "el estado cambió" sin modificar nada
  Y las dos TUIs muestran la integración

**Escenario: Sin worktree con la base sacada, avance rápido**

Dado ningún worktree con "main" sacada y "feat-pagos" que desciende de la punta de "main"
Cuando el desarrollador integra "feat-pagos"
Entonces "main" apunta al último commit de "feat-pagos" y ningún worktree cambia

**Escenario: Sin worktree con la base sacada y sin avance rápido**

Dado ningún worktree con "main" sacada y "feat-pagos" divergente de "main"
Cuando el desarrollador pide integrar "feat-pagos"
Entonces se rechaza con "saca main en un worktree para integrar" y nada cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). ADR-CKP-002 § 5 y § 7._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConfirmPrompt).
- **Dev Spec:** pendiente.
