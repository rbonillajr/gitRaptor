---
id: US-MCP-013
title: "Un agente sabe que una acción necesita al desarrollador y a quién acudir"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-002
    - ADR-GRD-003
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-009
    - US-MCP-014
    - US-GRD-007
ado:
  id: null
  url: null
covers: [BR-MCP-WF-004]
blocked_by: [ADR-MCP-001, ADR-CKP-002]
tags: [mcp, pedir-confirmacion, guardrails, ola-3]
---

# US-MCP-013: Un agente sabe que una acción necesita al desarrollador y a quién acudir

## Descripción

**Como** desarrollador orquestador, **quiero** que, mientras no exista la cola de confirmación, una operación que Guardrails marca como "pedir confirmación" se rechace por MCP diciendo que requiere mi confirmación y dónde dármela, **para** que ningún agente ejecute una acción de riesgo que yo no aprobé ni se quede reintentando sin saber por qué.

**Valor**: la regla "pedir confirmación" del equipo se respeta por MCP desde la primera escritura, sin esperar a la cola (Q-MCP-12).

## Reglas cubiertas

BR-MCP-WF-004 (variante "denegar" mientras no hay cola; nunca elicitation como confirmación) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-009 (flujo de escritura). De Guardrails: US-GRD-007 (permiso "pedir confirmación" por operación).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe) y ADR-CKP-002 (**propuesto**, por la operación `commit`; DEP-MCP-5 vía ADR-CKP-002): bloqueos de arquitectura. La acción remite a GitRaptor (Cockpit, F-001-02) solo como texto.
- **Relación**: la variante "pendiente con id" es US-MCP-014, bloqueada por la cola (US-GRD-015).

## Criterios de Aceptación

**Escenario: Una operación que requiere confirmación se rechaza con la acción**

Dado el repo "shop" cuya configuración del equipo fija "commit" en "pedir confirmación"
  Y la cola de confirmación no disponible
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces la petición se rechaza con el motivo "requiere confirmación del desarrollador"
  Y la acción indica que el desarrollador la hace desde GitRaptor
  Y el worktree no cambia
  Y la decisión queda registrada una sola vez con capa "mcp"

**Escenario: El servidor nunca pide la confirmación al cliente del agente**

Dado el repo "shop" cuya configuración del equipo fija "commit" en "pedir confirmación"
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces el servidor no pide ninguna confirmación al cliente del agente
  Y la respuesta llega sin esperar a nadie

**Escenario: Reintentar no cambia la decisión**

Dado "claude-1" recibió "requiere confirmación del desarrollador" para un `safe_commit` de "src/a.rs"
Cuando repite la misma petición
Entonces recibe el mismo rechazo con el mismo motivo
  Y el worktree sigue sin cambios

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001).
