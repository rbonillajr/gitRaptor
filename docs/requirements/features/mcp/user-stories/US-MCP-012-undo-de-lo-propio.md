---
id: US-MCP-012
title: "Un agente deshace por MCP su última operación sin tocar el trabajo de otros"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-09
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-TMC-005
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-007
    - US-MCP-009
    - US-TMC-002
    - US-TMC-012
    - US-TMC-013
    - US-TMC-021
ado:
  id: null
  url: null
covers: [BR-MCP-ELIG-005, BR-MCP-AUTH-003, BR-MCP-EDGE-003, BR-MCP-AUTH-002]
blocked_by: []
tags: [mcp, undo, time-machine, solape, ola-3]
---

# US-MCP-012: Un agente deshace por MCP su última operación sin tocar el trabajo de otros

## Descripción

**Como** desarrollador orquestador, **quiero** que un agente atribuido deshaga por MCP su última operación en su worktree, y que se detenga si eso tocaría el trabajo de otro actor, **para** que el agente corrija sus propios errores en segundos sin que la red de seguridad borre el trabajo de nadie más.

**Valor**: undo universal al alcance del agente (BR-14) sin pérdida de datos (NFR-01); los undos por MCP cuentan en el KPI de undos (BRD § 9).

## Reglas cubiertas

BR-MCP-ELIG-005 (parte: `undo` de la última operación propia; redo y restaurar no existen por MCP) · BR-MCP-AUTH-003 (parte: solo deshace lo suyo) · BR-MCP-EDGE-003 (parte: solape con otro actor → se detiene) · BR-MCP-AUTH-002 ("sin atribuir" nunca deshace, TQ-7) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-007 (requisito previo de toda escritura), US-MCP-009 (operaciones propias que deshacer). De la Time Machine: US-TMC-002 (undo y pila por worktree), US-TMC-012 (solape), US-TMC-013 (permisos del solicitante; espera esta feature para el canal MCP); US-TMC-019 (un undo por MCP debe entregar los avisos pendientes de `timemachine.notices` del worktree del agente, con forma acotada y reconocidos en una segunda llamada, según ADR-MCP-001; deuda anotada el 2026-10-09).
- **Relación (Fase 2)**: US-TMC-021 (políticas de Guardrails que restringen el undo). No es requisito previo: en el MVP el `undo` no pasa por Guardrails (D-18, BR-MCP-001 v0.3). Mientras tanto, por MCP se rechaza el undo que movería la rama base confirmada o una ref protegida (BR-MCP-ELIG-005, S-04; D-26).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): bloqueo de arquitectura. El `undo` no es una operación del catálogo de ADR-CKP-002: lo gobierna la Time Machine (ADR-TMC-005).

## Criterios de Aceptación

**Escenario: El agente deshace su último commit**

Dado "claude-1" atribuido en "shop-feat-a", cuya última operación es su commit "9f8e7d6" hecho por MCP
Cuando "claude-1" pide `undo`
Entonces "shop-feat-a" vuelve al estado previo a ese commit
  Y el timeline registra el undo con actor "claude-1" y canal "mcp"

**Escenario: La última operación es de otro agente**

Dado "claude-1" y "claude-3" en "shop-feat-a", y la última operación del worktree es un commit de "claude-3"
Cuando "claude-1" pide `undo`
Entonces la petición se rechaza con el motivo "la última operación es de claude-3; solo puedes deshacer lo tuyo"
  Y el worktree no cambia

**Escenario: El undo se detiene ante el trabajo posterior de otro actor**

Dado la última operación de "shop-feat-a" es de "claude-1"
  Y después "claude-3" modificó las mismas líneas de "src/pago.rs"
Cuando "claude-1" pide `undo`
Entonces el undo se detiene y la respuesta informa del solape con "claude-3" en "src/pago.rs"
  Y el worktree no cambia

**Escenario: Sin atribuir nunca deshace**

Dado un cliente MCP resuelto como "sin atribuir" en "shop-feat-a"
Cuando pide `undo`
Entonces la petición se rechaza con la acción "usa register_agent"
  Y el worktree no cambia

**Escenario: No hay nada que deshacer**

Dado "shop-feat-a" sin operaciones de "claude-1" en su pila
Cuando "claude-1" pide `undo`
Entonces la petición se rechaza con el motivo "nada que deshacer"

**Escenario: Rehacer y restaurar no existen para los agentes**

Dado una sesión MCP abierta con el servidor de GitRaptor
Cuando el agente busca en la lista de herramientas una que rehaga o restaure a un punto
Entonces no existe ninguna
  Y esas acciones solo están en la CLI y la TUI del desarrollador

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001).
