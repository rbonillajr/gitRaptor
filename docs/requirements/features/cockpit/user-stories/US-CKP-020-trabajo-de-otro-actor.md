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
    - US-CKP-015
    - US-CKP-017
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

- **Historias**: US-CKP-014, US-CKP-015 y US-CKP-017; US-TMC-013 (permisos por solicitante).
- **Técnicas**: TS-CKP-002, TS-TMC-004 (reto ligado al plan).

## Criterios de Aceptación

**Escenario: Confirmar el plan concreto (macOS)**

Dado "feat-wip" con trabajo de "claude-2" y la TUI del desarrollador como "Tú u otro (sin atribuir)"
Cuando el desarrollador pide descartar "feat-wip"
Entonces la TUI pide confirmar el plan "borrar worktree feat-wip y rama feat-wip"
  Y solo tras confirmarlo se ejecuta

**Escenario: Si el plan cambia, la confirmación no vale**

Dado la confirmación del plan de integrar "feat-wip" hasta "a1b2c3"
Cuando "claude-2" commitea "d4e5f6" antes de ejecutar
Entonces la operación se rechaza con "el estado cambió" y nada cambia

**Escenario: Rebase sobre trabajo de otro actor**

Dado "feat-wip" con commits de "claude-2", su sesión Terminado
Cuando el desarrollador pide rebasar "feat-wip"
Entonces la TUI pide confirmar el plan del rebase antes de ejecutarlo

**Escenario: Un agente no toca el trabajo de otro**

Dado la TUI abierta desde el terminal de "claude-1"
Cuando pide descartar "feat-wip" de "claude-2"
Entonces se rechaza y nada cambia

**Escenario: En Windows, rechazo**

Dado el desarrollador en Windows
Cuando pide descartar "feat-wip" de "claude-2"
Entonces se rechaza con "no se puede confirmar trabajo de otro actor en Windows todavía"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). ADR-TMC-005 § 2-3; ADR-CKP-002 § 3._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConfirmPrompt).
- **Dev Spec:** pendiente. Escenario de Windows: Pendiente: etapa de validación multiplataforma.
