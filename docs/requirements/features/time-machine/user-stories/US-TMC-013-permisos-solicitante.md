---
id: US-TMC-013
title: "Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-GRP-007, US-GRP-009]
covers: [BR-TMC-AUTH-001, D-TMC-6, D-TMC-17, D-TMC-23]
blocked_by: []
tags: [time-machine, permisos, solicitante]
---

# US-TMC-013: Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell

## Descripción

**Como** desarrollador orquestador
**Quiero** que cada petición de undo se atribuya como un evento y que tocar trabajo de otro actor exija mi confirmación interactiva
**Para** que un agente no pueda borrar el trabajo de otro usando la propia red de seguridad

**Valor**: GitRaptor no puede probar quién es humano (Q34); este control evita que esa limitación se convierta en un riesgo (R7).

## Reglas cubiertas

BR-TMC-AUTH-001 (solicitante atribuido; confirmación interactiva) · D-TMC-6, D-TMC-17, D-TMC-23 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Un agente deshace su propia operación**

Dado un commit de "claude-1" en "feat-login"
Cuando "claude-1" pide por MCP deshacerlo
Entonces el commit se deshace
  Y el registro del undo indica como solicitante a "claude-1"

**Escenario: Un agente no deshace la operación de otro**

Dado un commit de "claude-2"
Cuando "claude-1" pide deshacerlo, por MCP o con la CLI desde su propia shell
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

**Escenario: Un solicitante sin atribuir necesita confirmación para tocar trabajo ajeno**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo
  Y el desarrollador lo confirma de forma interactiva en ese momento
Entonces el commit se deshace
  Y el registro del undo indica como solicitante "sin atribuir"

**Escenario: Sin confirmación no hay undo**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo sin poder confirmar de forma interactiva
Entonces la petición se rechaza
  Y el repo no cambia

**Escenario: Otro agente registrado sigue la misma regla**

Dado "codex-1" registrado como otro agente
Cuando "codex-1" pide deshacer un commit de "claude-1"
Entonces la petición se rechaza
  Y el repo no cambia


## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002; US-GRP-007 y US-GRP-009 de motor-local.
- **Externas**: F-001-05 Servidor MCP (canal `undo` para agentes). Las restricciones adicionales de Guardrails están en US-TMC-021. Cómo se identifica al solicitante lo decide el Arquitecto (D-TMC-23).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
