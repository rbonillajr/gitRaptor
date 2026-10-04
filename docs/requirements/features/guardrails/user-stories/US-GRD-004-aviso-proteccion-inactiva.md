---
id: US-GRD-004
title: "El desarrollador se entera de que la protección de un repo dejó de estar activa"
type: us
status: draft
priority: medium
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
    - US-GRP-006
tags:
  - guardrails
  - estado-proteccion
  - hooks-git
  - limites
---

# US-GRD-004: El desarrollador se entera de que la protección de un repo dejó de estar activa

## Descripción

**Como** desarrollador orquestador, **quiero** saber cuándo la protección de hooks de un repo deja de funcionar por una causa ajena a Guardrails y qué operaciones no puede impedir, **para** no creerme protegido cuando no lo estoy.

**Valor**: el estado de protección nunca miente (BR-WF-002, Q-GRD-8).

## Reglas cubiertas

BR-WF-002 (hooks inactivos por otra causa → aviso; repo retirado de la observación, Q-GRD-15) · BR-EDGE-003 (lista publicada de operaciones que la capa de hooks no puede impedir) · BR-EDGE-001 (el mínimo seguro es visible para el desarrollador) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (estado "Solo hooks"); US-GRP-006 (motor-local: retirar un repo de la observación).
- **Externas**: ninguna. Los estados que dependen de la allowlist del MCP están en US-GRD-016.
- **Transversal**: Windows, macOS y Linux. La presentación del aviso es del Cockpit y la CLI.

## Criterios de Aceptación

**Escenario: Otro gestor reemplaza la protección**

Dado el repo "demo" en estado "Solo hooks"
Cuando otra herramienta reemplaza los hooks de Guardrails en "demo"
Entonces el estado de protección de "demo" pasa a "Sin protección"
  Y GitRaptor avisa al desarrollador de que la protección dejó de estar activa
  Y el cambio de estado queda en el registro

**Escenario: Retirar la protección desde Guardrails no genera aviso**

Dado el repo "demo" en estado "Solo hooks"
Cuando el desarrollador retira la protección desde Guardrails
Entonces el estado pasa a "Sin protección" sin aviso de pérdida

**Escenario: Retirar el repo de la observación no desactiva sus hooks**

Dado el repo "demo" en estado "Solo hooks"
Cuando el desarrollador retira "demo" de la observación del motor
Entonces los hooks de Guardrails siguen denegando el force-push en "demo", con actor "sin atribuir"
  Y GitRaptor avisa de que "demo" sigue protegido aunque ya no se observa

**Escenario: El desarrollador ve qué reglas aplican sin configuración**

Dado el repo "demo" en estado "Solo hooks" y sin configuración de Guardrails
Cuando el desarrollador consulta el estado de protección de "demo"
Entonces obtiene que aplica el conjunto mínimo por defecto: force-push denegado y borrado de la rama base denegado
  Y que el equipo lo puede desactivar en su configuración

**Escenario: El desarrollador consulta qué no se puede impedir**

Dado el repo "demo" en estado "Solo hooks"
Cuando el desarrollador consulta el estado de protección de "demo"
Entonces obtiene la lista de operaciones del catálogo que la capa de hooks no puede impedir con Git directo
  Y la lista coincide con lo que se observa al probar esas operaciones con Git directo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (presentación del Cockpit y la CLI).
- **Dev Spec:** pendiente (Arquitecto).
