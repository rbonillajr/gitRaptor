---
id: US-GRD-015
title: "El desarrollador aprueba o rechaza las acciones de riesgo que un agente deja en espera"
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
    - US-GRD-004
    - US-GRD-005
    - US-GRD-007
tags:
  - guardrails
  - pedir-confirmacion
  - cola
  - should
  - bloqueada
---

# US-GRD-015: El desarrollador aprueba o rechaza las acciones de riesgo que un agente deja en espera

## Descripción

**Como** desarrollador orquestador, **quiero** que una operación marcada como "pedir confirmación" espere mi decisión hasta 5 minutos y que solo yo pueda aprobarla o rechazarla, **para** dejar que los agentes avancen en lo arriesgado sin perder la última palabra.

**Valor**: un punto intermedio entre permitir y denegar (BRD BR-13, Should).

## Reglas cubiertas

BR-WF-001 (pendiente, aprobada, rechazada, caducada; finales; una aprobación vale una vez, S-GRD-8) · BR-TIME-001 (plazo de 5 minutos; en Git directo la operación espera, Q-GRD-6) · BR-AUTH-001 (solo el humano decide) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (permiso "pedir confirmación"; esta historia sustituye el trato como denegar), US-GRD-005 (registro de cada transición).
- **Externas**: **bloqueada** por el Cockpit F-001-02 (superficie donde el humano decide; Q-GRD-13) y por el gate de Q-GRD-19 de abajo. P8 ya no la bloquea (ADR-GRP-007, aceptado el 2026-10-04). **Gate de Q-GRD-19**: aprobar una petición exige el factor de autenticación del sistema operativo, fuera del canal del agente; esta historia no se empieza sin él. Distinguir al humano: transversal (lo define el Arquitecto; R-GRD-3).
- **Transversal**: Windows, macOS y Linux. Los escenarios usan push con Git directo, una operación interceptable según la lista de US-GRD-004. La cola por la capa MCP sigue la misma decisión (BR-CONS-002) y se verifica cuando exista F-001-05 (US-GRD-016); por eso esta historia no depende del MCP.

## Criterios de Aceptación

**Escenario: El humano aprueba y la operación se ejecuta una vez (capa de hooks, Git directo)**

Dado el repo "demo" protegido con push en "pedir confirmación"
Cuando el agente "codex" hace push de "feat-x" con Git directo, la operación espera, y el desarrollador aprueba la petición a los 40 segundos
Entonces el push de "feat-x" se ejecuta
  Y la petición queda "aprobada" con su operación, repo, worktree, actor, regla y momentos

**Escenario: El humano rechaza (capa de hooks, Git directo)**

Dado una petición pendiente de "codex" por un push de "feat-y" con Git directo
Cuando el desarrollador la rechaza
Entonces la petición queda "rechazada", el remoto de "feat-y" no cambia y "codex" recibe el motivo

**Escenario: Sin respuesta en 5 minutos, caduca (capa de hooks, Git directo)**

Dado una petición pendiente por un push con Git directo, creada a las 10:00:00
Cuando son las 10:05:00 sin decisión
Entonces la petición queda "caducada" y la operación no se ejecuta
  Y una aprobación a las 10:05:30 no ejecuta nada

**Escenario: Un agente no puede aprobar (desde el canal del agente)**

Dado una petición pendiente de "codex" por un push con Git directo
Cuando "codex" u otro agente intentan aprobarla desde su canal
Entonces la aprobación se rechaza y la petición sigue pendiente

**Escenario: Reintentar crea una petición nueva (capa de hooks, Git directo)**

Dado una petición de "codex" por un push con Git directo, ya rechazada
Cuando "codex" vuelve a hacer ese push
Entonces se crea una petición nueva pendiente y la rechazada no cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** la presentación de la cola es del Cockpit (F-001-02).
- **Dev Spec:** pendiente (Arquitecto).
