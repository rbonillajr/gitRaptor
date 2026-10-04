---
id: US-GRD-017
title: "Una operación destructiva permitida no se ejecuta sin un punto de recuperación"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
tags:
  - guardrails
  - snapshot
  - nfr-01
  - time-machine
  - bloqueada
---

# US-GRD-017: Una operación destructiva permitida no se ejecuta sin un punto de recuperación

## Descripción

**Como** desarrollador orquestador, **quiero** que una operación destructiva que las reglas permiten solo se ejecute si antes se guardó un punto de recuperación, **para** poder deshacer siempre lo que un agente destruya con permiso.

**Valor**: permitir nunca significa perder trabajo (NFR-01, Q-GRD-11).

## Reglas cubiertas

BR-EDGE-005 (sin snapshot previo, se deniega con motivo; el humano puede usar la excepción consciente) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (decisión).
- **Externas**: **bloqueada** por la Time Machine F-001-03 (snapshot previo, BR-08). Guardrails no toma el snapshot.
- **Transversal**: Windows, macOS y Linux; pruebas de caos (NFR-12). Los escenarios usan una operación que la capa de hooks puede interceptar; `reset --hard` con Git directo no lo es (BR-EDGE-003) y por MCP se cubre cuando exista F-001-05.

## Criterios de Aceptación

**Escenario: Con snapshot, la operación permitida se ejecuta**

Dado el repo "demo" protegido, donde borrar ramas está permitido, y la rama "feat-x" con 3 commits que no están en ninguna otra rama
Cuando un proceso borra "feat-x" con Git directo y la Time Machine guarda antes el punto de recuperación
Entonces la operación se ejecuta
  Y los 3 commits de "feat-x" se pueden recuperar desde ese punto

**Escenario: Sin snapshot, la operación se deniega**

Dado el repo "demo" donde borrar ramas está permitido, y la rama "feat-x" con commits que no están en ninguna otra rama
Cuando un proceso borra "feat-x" y la Time Machine no puede guardar el punto de recuperación
Entonces la operación no se ejecuta
  Y el motivo dice que no se pudo guardar un punto de recuperación
  Y "feat-x" sigue existiendo con sus commits

**Escenario: El humano decide seguir sin punto de recuperación**

Dado la misma situación sin snapshot posible
Cuando el desarrollador usa la excepción consciente para esa operación
Entonces la operación se ejecuta y la excepción queda en el registro

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, coordinado con F-001-03).
