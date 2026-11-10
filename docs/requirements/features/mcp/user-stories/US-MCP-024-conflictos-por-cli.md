---
id: US-MCP-024
title: "Un agente que trabaja por Bash sabe con quién va a chocar antes de integrar"
type: us
status: draft
priority: low
created: 2026-11-10
updated: 2026-11-10
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-CKP-001
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-016
    - US-MCP-020
    - US-MCP-023
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-008]
blocked_by: []
tags: [mcp, agentes, cli, json, conflictos, superficie-agentes, post-v0.1.0]
---

# US-MCP-024: Un agente que trabaja por Bash sabe con quién va a chocar antes de integrar

## Descripción

**Como** desarrollador orquestador, **quiero** que la CLI `raptor` dé con `--json` la misma predicción de conflictos que la herramienta `check_conflicts` del MCP, **para** que un agente que trabaja por Bash también pregunte con quién va a chocar antes de integrar, como le pide la guía.

**Valor**: la guía (US-MCP-020) pide preguntar antes de integrar por cualquiera de los dos caminos; hoy la predicción solo sale por el MCP y por la TUI ([inventario](../research/inventario-json-cli-2026-11.md)).

**Prioridad e hito**: Could, después de v0.1.0. Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32 y P-MCP-37).

## Reglas cubiertas

BR-MCP-CONS-008 (parte: un subcomando más con el contrato `--json`, vista del perfil `mcp`) — ver [business-rules.md](../business-rules.md). El contenido es el de `check_conflicts` (BR-MCP-CALC-004), que ya entrega US-MCP-016.

## Dependencias

- **Historias**: US-MCP-016 (la predicción publicada que se expone) y US-MCP-023 (contrato `--json` versionado).
- **Relación**: US-MCP-020 (la guía nombra este camino).

## Criterios de Aceptación

**Escenario: El agente ve por la CLI los choques de su worktree**

Dado "claude-1" en "shop-feat-a" y "claude-2" en "shop-feat-b" modifican "src/cart.rs" en rangos de líneas que se solapan
Cuando "claude-1" pide por la CLI, con `--json`, la predicción de conflictos de su worktree
Entonces la salida nombra el par "shop-feat-a" y "shop-feat-b", el nivel, la ruta "src/cart.rs" y los rangos de líneas
  Y no contiene contenido de ningún archivo
  Y declara la versión del contrato y la antigüedad de la predicción

**Escenario: Misma respuesta por la CLI y por el MCP**

Dado la misma predicción publicada para "shop-feat-a"
Cuando "claude-1" la pide por la CLI y por `check_conflicts`
Entonces los pares, niveles, rutas y rangos coinciden

**Escenario: El agente puede pedir los choques de todo el repo**

Dado tres worktrees de "shop" con dos pares en riesgo, uno de ellos sin "shop-feat-a"
Cuando "claude-1" pide la predicción de todo el repo
Entonces la salida incluye los dos pares

**Escenario: La predicción todavía se está calculando**

Dado la predicción de "shop" en cálculo tras un cambio reciente
Cuando "claude-1" la pide por la CLI
Entonces la salida declara el estado "calculando" con la antigüedad del último resultado
  Y no presenta como actual un resultado anterior

**Escenario: Un repo no habilitado no da datos**

Dado el repo "otro" observado pero no habilitado para el MCP
Cuando un agente pide por la CLI la predicción de "otro"
Entonces la salida es el rechazo "repo-not-enabled" con su acción, en JSON
  Y no contiene ningún par ni ruta de "otro"

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), puntos A y D.

- **Sin contrato nuevo**: el subcomando consulta el mismo método del canal que `check_conflicts` (ADR-CKP-001 § 8, US-MCP-016), con la vista `mcp` del solicitante agente (C1). Sale con el sobre y la versión de US-MCP-023.
- **Sin herramienta MCP nueva**: la predicción ya está en el MCP; esto es el segundo camino.

## Diseño y Dev Spec

- **Diseño:** no aplica (salida para máquinas).
- **Dev Spec:** **no** (decisión del Arquitecto, 2026-11-10): reutiliza la consulta de US-MCP-016 y el contrato de US-MCP-023; estos Requisitos Técnicos bastan.
