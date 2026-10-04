---
id: US-TMC-021
title: "Las políticas del repo pueden restringir quién deshace, nunca ampliarlo"
type: us
status: draft
priority: low
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
  stories: [US-TMC-013]
covers: [BR-TMC-AUTH-001, D-TMC-17]
blocked_by: ["F-001-04 (políticas de Guardrails)"]
tags: [time-machine, permisos, guardrails]
---

# US-TMC-021: Las políticas del repo pueden restringir quién deshace, nunca ampliarlo

## Descripción

**Como** desarrollador orquestador
**Quiero** que una política de Guardrails pueda restringir aún más quién deshace en mi repo
**Para** adaptar la red de seguridad a las reglas del equipo sin abrir la puerta a que un agente borre trabajo ajeno

**Valor**: Guardrails endurece los permisos del undo; nunca los relaja (D-TMC-17).

## Reglas cubiertas

BR-TMC-AUTH-001 (Guardrails puede restringir, nunca ampliar) · D-TMC-17 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Una política impide a los agentes deshacer**

Dado una política del repo que prohíbe a los agentes deshacer
Cuando "claude-1" pide deshacer su propio commit
Entonces la petición se rechaza indicando la política
  Y el repo no cambia

**Escenario: Una política no puede ampliar los permisos**

Dado una política del repo que dice permitir a "claude-1" deshacer trabajo de "claude-2"
Cuando "claude-1" pide deshacer un commit de "claude-2"
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

**Escenario: Sin política se aplica la regla base**

Dado un repo sin políticas sobre el undo
Cuando "claude-1" pide deshacer su propio commit
Entonces el commit se deshace

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-013.
- **Externas**: **bloqueada** hasta que existan las políticas por repo de Guardrails (F-001-04, BR-11) y su formato.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
