---
id: US-MCP-015
title: "El desarrollador mide cuánto paró y cuánto recuperó el MCP"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-GRD-006
    - ADR-GRP-013
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-009
    - US-MCP-012
    - US-MCP-013
    - US-GRD-005
    - US-TMC-006
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-006]
blocked_by: []
tags: [mcp, kpi, medicion, ola-3]
---

# US-MCP-015: El desarrollador mide cuánto paró y cuánto recuperó el MCP

## Descripción

**Como** desarrollador orquestador, **quiero** contar por capa las acciones que Guardrails bloqueó, los undos hechos por MCP y los indicadores de seguridad del MCP, contando solo sus herramientas, **para** saber si el MCP cumple sus metas de cero y decidir si mis agentes deberían usarlo más.

**Valor**: los KPIs del § 3 del contexto y del BRD § 9 se pueden medir (Q-MCP-18).

## Reglas cubiertas

BR-MCP-CONS-006 (decisión registrada una vez con capa `mcp`; KPIs solo de las herramientas; bloqueos por capa; undos por MCP en el KPI de undos) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-009 (escrituras), US-MCP-012 (undos), US-MCP-013 (denegaciones por "pedir confirmación"). De Guardrails: US-GRD-005 (registro de bloqueos por repo). De la Time Machine: US-TMC-006 (timeline con actor).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): bloqueo de arquitectura.
- **Relación**: la demo del BRD § 13 en dos partes es la prueba de aceptación de la feature, no un escenario de esta historia (ver el índice).

## Criterios de Aceptación

**Escenario: Los bloqueos se cuentan por capa**

Dado en el último mes Guardrails bloqueó en "shop" 12 operaciones pedidas por MCP y 7 por Git directo
Cuando el desarrollador consulta las acciones bloqueadas de "shop"
Entonces ve "12 por mcp, 7 por hooks"

**Escenario: Una decisión del MCP se cuenta una sola vez**

Dado un `safe_commit` de "claude-1" denegado por Guardrails
Cuando el desarrollador consulta el registro de decisiones
Entonces hay una sola entrada de esa decisión, con capa "mcp" y actor "claude-1"

**Escenario: Lo que el agente hace con Git crudo no cuenta como operación del MCP**

Dado "claude-1" con el servidor MCP instalado hace un commit con Git directo en "shop-feat-a"
Cuando el desarrollador consulta las operaciones hechas por MCP
Entonces ese commit no figura entre ellas
  Y figura en el timeline como observado por el motor

**Escenario: Los undos del agente cuentan en el indicador de undos**

Dado "claude-1" hizo 3 undos por MCP este mes
Cuando el desarrollador consulta los undos por usuario activo del mes
Entonces esos 3 undos están incluidos y se distinguen por canal "mcp"

**Escenario: Los indicadores de seguridad del MCP se pueden consultar**

Dado un mes de uso del MCP en "shop"
Cuando el desarrollador consulta los indicadores del MCP
Entonces ve las operaciones denegadas que se ejecutaron, las escrituras sin snapshot, las respuestas con datos de repos fuera de la allowlist, las escrituras fuera del worktree del llamante y los comandos reservados aceptados desde procesos lanzados por una operación
  Y cada indicador cuenta solo lo hecho con las herramientas del MCP
  Y la adopción del MCP se informa como porcentaje, sin meta

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (consulta por CLI; presentación en el Cockpit, F-001-02).
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001).
