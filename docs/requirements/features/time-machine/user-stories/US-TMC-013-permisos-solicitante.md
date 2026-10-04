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
**Quiero** que cada petición de undo se atribuya como un evento y que tocar trabajo de otro actor exija mi confirmación interactiva, o se rechace donde esa confirmación no se puede probar
**Para** que un agente no pueda borrar el trabajo de otro usando la propia red de seguridad

**Valor**: GitRaptor no puede probar quién es humano (Q34); este control evita que esa limitación se convierta en un riesgo (R7).

## Reglas cubiertas

BR-TMC-AUTH-001 (solicitante atribuido; confirmación interactiva en macOS y Linux; rechazo en Windows y por MCP) · D-TMC-6, D-TMC-17, D-TMC-23 (actualizada por TQ-14 y TQ-7) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Un agente deshace su propia operación**

Dado un commit de "claude-1" en "feat-login"
Cuando "claude-1" pide por MCP deshacerlo
Entonces el commit se deshace
  Y el registro del undo indica como solicitante a "claude-1"

**Esquema del escenario: Un agente no deshace la operación de otro**

Dado un commit de "claude-2"
  Y "codex-1" registrado como otro agente
Cuando "<agente>" pide deshacerlo por <canal>
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

Ejemplos:

| agente | canal |
|--------|-------|
| claude-1 | MCP |
| claude-1 | la CLI desde su propia shell |
| codex-1 | MCP |

**Escenario: En macOS y Linux, un solicitante sin atribuir confirma para tocar trabajo ajeno**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo con la CLI en macOS o Linux
  Y el desarrollador lo confirma de forma interactiva en ese momento
Entonces el commit se deshace
  Y el registro del undo indica como solicitante "sin atribuir"

**Escenario: Sin confirmación no hay undo**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo con la CLI en macOS o Linux sin poder confirmar de forma interactiva
Entonces la petición se rechaza
  Y el repo no cambia

**Escenario: En Windows, un solicitante sin atribuir no puede tocar trabajo ajeno**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo con la CLI en Windows
Entonces la petición se rechaza sin pedir confirmación
  Y el solicitante recibe el motivo: en Windows todavía no se puede confirmar trabajo de otro actor
  Y el repo no cambia

**Escenario: Por MCP, un solicitante sin atribuir se rechaza siempre**

Dado una edición "sin atribuir" en "feat-login"
Cuando un solicitante "sin atribuir" pide deshacerla por MCP
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

## Requisitos Técnicos

- Dueña de la regla base de permisos y de la confirmación (ADR-TMC-005 § 2-3): agente X solo lo suyo; "sin atribuir" necesita confirmación para trabajo de un agente.
- La confirmación reutiliza los controles del daemon de ADR-GRP-005 § 6 (identificador no reutilizable, ascendencia, terminal y líder de sesión); el MCP nunca la ofrece.
- "Sin atribuir" por MCP: rechazo incondicional y previo. Agentes registrados: reconocidos por la identidad de proceso guardada al registrarse; depende de que motor-local aplique la nota a ADR-GRP-012/013 (ADR-TMC-005 § 1).
- Toda petición, aceptada o rechazada, queda en el oplog con su motivo (SEC-TMC-03).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002; US-GRP-007 y US-GRP-009 de motor-local.
- **Externas**: F-001-05 Servidor MCP (canal `undo` para agentes). Las restricciones adicionales de Guardrails están en US-TMC-021. Cómo se identifica al solicitante lo decide el Arquitecto (D-TMC-23).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux salvo la confirmación interactiva, que en el MVP no existe en Windows (D-TMC-23); mensajes en inglés y español.
