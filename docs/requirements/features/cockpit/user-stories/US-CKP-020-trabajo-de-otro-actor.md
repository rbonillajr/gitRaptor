---
id: US-CKP-020
title: "Tocar el trabajo de otro actor exige confirmar el plan concreto"
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
    - US-TMC-013
tags:
  - cockpit
  - solicitante
  - trabajo-de-otro-actor
  - must
---

# US-CKP-020: Tocar el trabajo de otro actor exige confirmar el plan concreto

## Descripción

**Como** desarrollador orquestador, **quiero** que integrar, rebasar o descartar el trabajo de otro actor me pida confirmar exactamente qué se va a hacer, y que un agente nunca pueda hacerlo, **para** que nadie toque el trabajo de un agente por accidente o en su nombre.

**Valor**: BR-07 (Must); NFR-01. Extiende al Cockpit la regla de permisos de la Time Machine.

## Reglas cubiertas

BR-CKP-AUTH-003 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 (fija la confirmación del plan en el camino feliz; 015 y 017 la reutilizan en el suyo); US-TMC-013 (permisos por solicitante).
- **Alcance**: esta historia cubre los casos límite de AUTH-003 con el merge; el camino feliz con rebase y descarte lo verifican US-CKP-015 y US-CKP-017.
- **Técnicas**: TS-CKP-002, TS-TMC-004 (reto ligado al plan).

## Criterios de Aceptación

**Escenario: Trabajo sin atribuir, sin confirmación de plan**

Dado "feat-docs" con commits "sin atribuir" y la TUI del desarrollador como "Tú u otro (sin atribuir)"
Cuando el desarrollador integra "feat-docs"
Entonces no se pide confirmar el plan por trabajo ajeno

**Escenario: Atribución mixta cuenta como otro actor**

Dado "feat-mix" con commits de "claude-2" y commits "sin atribuir"
Cuando el desarrollador pide integrar "feat-mix"
Entonces la TUI pide confirmar el plan, como con trabajo de un agente

**Escenario: Si el plan cambia, la confirmación no vale**

Dado la confirmación del plan de integrar "feat-wip" hasta "a1b2c3"
Cuando "claude-2" commitea "d4e5f6" antes de ejecutar
Entonces la operación se rechaza con "el estado cambió" y nada cambia

**Escenario: Un agente no toca el trabajo de otro**

Dado la TUI abierta desde el terminal de "claude-1"
Cuando pide integrar "feat-wip" de "claude-2" por el canal
Entonces se rechaza y nada cambia

**Escenario: En Windows, rechazo** (Pendiente: etapa de validación multiplataforma)

Dado el desarrollador en Windows, donde el Cockpit todavía no tiene capa `cockpit` (ADR-CKP-002 § 4)
Cuando pide integrar "feat-wip" de "claude-2"
Entonces se rechaza con "no se puede confirmar trabajo de otro actor en Windows todavía" y nada cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). ADR-TMC-005 § 2-3; ADR-CKP-002 § 3._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConfirmPrompt).
- **Dev Spec:** pendiente. Escenario de Windows: Pendiente: etapa de validación multiplataforma.
