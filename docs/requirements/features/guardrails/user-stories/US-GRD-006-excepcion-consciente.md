---
id: US-GRD-006
title: "El desarrollador hace a conciencia una operación prohibida y queda constancia"
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
    - US-GRD-005
    - US-GRP-007
    - US-GRP-009
tags:
  - guardrails
  - excepcion-consciente
  - actores
  - sin-atribuir
---

# US-GRD-006: El desarrollador hace a conciencia una operación prohibida y queda constancia

## Descripción

**Como** desarrollador orquestador, **quiero** que las reglas se apliquen a cualquier actor, sea un agente o "sin atribuir", y tener una vía consciente y registrada para saltarme una regla en una operación concreta, **para** que un agente sin registrar no se cuele haciéndose pasar por mí sin que yo pierda el control de mi repo.

**Valor**: fail-safe sin dejar atrapado al humano (Q-GRD-1).

## Reglas cubiertas

BR-AUTH-003 (misma decisión para "agente X" y "sin atribuir"; excepción consciente) · BR-AUTH-001 (un agente no usa la excepción) · BR-CONS-004 (la excepción queda registrada) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (decisión), US-GRD-005 (registro); US-GRP-007 y US-GRP-009 (motor-local: agente detectado y registrado).
- **Externas**: el mecanismo que asegura que un agente no puede dar la confirmación humana es transversal (lo define el Arquitecto; riesgo R-GRD-3).
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Esquema del escenario: La misma operación recibe la misma decisión sea cual sea el actor**

Dado el repo "demo" protegido
Cuando "<actor>" hace force-push de "feat-x" con Git directo
Entonces la operación se deniega con el mismo motivo

Ejemplos:
| actor |
| Claude Code (detectado) |
| codex (registrado) |
| un proceso sin atribuir |

**Escenario: El desarrollador usa la excepción consciente**

Dado el repo "demo" protegido, donde el force-push está denegado
Cuando el desarrollador confirma de forma consciente que quiere hacer ese force-push concreto
Entonces la operación se ejecuta
  Y el registro de "demo" anota la excepción con la operación, la regla saltada y el momento

**Escenario: La excepción vale para una sola operación**

Dado el desarrollador usó la excepción para un force-push en "demo"
Cuando otro proceso hace un segundo force-push
Entonces se deniega como antes

**Escenario: Un agente no puede usar la excepción**

Dado el agente "codex" registrado en "demo"
Cuando "codex" intenta usar la excepción consciente desde su canal
Entonces la excepción se rechaza, la operación no se ejecuta
  Y el intento queda en el registro

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (la forma de confirmar es del Cockpit y la CLI).
- **Dev Spec:** pendiente (Arquitecto).
