---
id: US-MCP-014
title: "La acción de riesgo de un agente queda en espera del desarrollador sin bloquear al agente"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-27
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-GRD-003
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-013
    - US-GRD-015
ado:
  id: null
  url: null
covers: [BR-MCP-WF-004]
blocked_by: [US-GRD-015, SPIKE-GRD-002, US-MCP-009]
tags: [mcp, pedir-confirmacion, cola, bloqueada]
---

# US-MCP-014: La acción de riesgo de un agente queda en espera del desarrollador sin bloquear al agente

## Descripción

**Como** desarrollador orquestador, **quiero** que, cuando exista la cola de confirmación, una operación "pedir confirmación" pedida por MCP quede en mi cola y el agente reciba "pendiente" con un id sin quedarse esperando, **para** aprobar o rechazar lo arriesgado cuando pueda, mientras el agente sigue con otra cosa.

**Valor**: el humano tiene la última palabra en lo arriesgado sin frenar al agente (BR-12, Q-MCP-12).

## Reglas cubiertas

BR-MCP-WF-004 (variante "pendiente" con id, sin bloquear; nunca elicitation) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-013 (variante "denegar", que sigue rigiendo cuando la cola no está disponible).
- **Relación**: US-CKP-023, la cola en el Cockpit (propuesta, rama docs/stories-cockpit), donde el desarrollador decide sobre la petición pendiente.
- **Externas**: **Bloqueada**: la cola de confirmación US-GRD-015 (Guardrails), que ya no depende del Cockpit F-001-02 y espera al factor de autenticación fuera de banda del SO (ADR-GRD-008, aceptado el 2026-10-04; SPIKE-GRD-002 sin ejecutar). DEP-MCP-7 (DEP-CKP-8). `safe_commit` (US-MCP-009, PR #289) aún no está en `main`. Lo que la cola debe ofrecer al MCP (R1–R10) y el fallo cerrado están en DS-US-MCP-014.

## Criterios de Aceptación

**Escenario: La petición queda pendiente y el agente sigue**

Dado el repo "shop" cuya configuración del equipo fija "commit" en "pedir confirmación"
  Y la cola de confirmación disponible
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces la respuesta es "pendiente" con un id de petición y llega sin esperar la decisión
  Y la cola del desarrollador contiene la petición con el agente "claude-1", la operación y el worktree
  Y el worktree no cambia hasta que el desarrollador decide

**Escenario: Ni con cola el servidor pide confirmación al cliente del agente**

Dado la cola de confirmación disponible
Cuando "claude-1" pide una operación marcada "pedir confirmación"
Entonces el servidor no pide ninguna confirmación al cliente del agente

**Escenario: Si la cola deja de estar disponible, se vuelve a denegar con acción**

Dado la cola de confirmación no disponible en este momento
Cuando "claude-1" pide `safe_commit` de "src/a.rs" con la regla "pedir confirmación"
Entonces la petición se rechaza con el motivo "requiere confirmación del desarrollador" y la acción
  Y no se crea ninguna petición pendiente

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica en el MCP; la cola la presenta el Cockpit (F-001-02).
- **Dev Spec:** [DS-US-MCP-014](../dev-specs/US-MCP-014-dev-spec.md) (2026-10-27).
