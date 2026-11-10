---
id: US-MCP-025
title: "El desarrollador ve cuánto usan sus agentes el MCP y cuánto la CLI"
type: us
status: draft
priority: medium
created: 2026-11-10
updated: 2026-11-10
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-015
    - US-MCP-023
    - US-MCP-026
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-010]
blocked_by: []
tags: [mcp, agentes, medicion, cli, superficie-agentes, hito-m4]
---

# US-MCP-025: El desarrollador ve cuánto usan sus agentes el MCP y cuánto la CLI

## Descripción

**Como** desarrollador orquestador, **quiero** contar por agente y por repo cuántas llamadas de mis agentes llegan por el MCP y cuántas por la CLI, por herramienta o subcomando, y cuántas terminan en rechazo, **para** decidir con datos qué herramientas del MCP se mantienen y cuáles sobran.

**Valor**: recortar herramientas del MCP quedó fuera de alcance hasta tener datos propios. El dato de Nx (88 % de acierto con skills y CLI frente a 81 % con MCP) es de terceros, sin verificar y medido con otra herramienta: no sirve de línea base.

**Prioridad e hito**: Should, M4 (v0.1.0). Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32).

## Reglas cubiertas

BR-MCP-CONS-010 (cada llamada de un agente contada una vez con canal, agente, repo, herramienta y resultado; sin meta; sin contenido) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-023 (los subcomandos de uso por agentes y su contrato) y US-MCP-015 (registro de decisiones del MCP con capa y actor).
- **Relación**: US-MCP-026 usa estos conteos en la comparación por tarea.

## Criterios de Aceptación

**Escenario: El uso se ve por canal, agente y repo**

Dado en el último mes "claude-1" hizo en "shop" 140 llamadas por el MCP y 60 por la CLI
  Y 9 de las del MCP y 2 de las de la CLI terminaron en rechazo
Cuando el desarrollador consulta el uso por canal de "shop"
Entonces ve "claude-1 en shop, último mes: 140 por mcp, 60 por cli; rechazos: 9 por mcp, 2 por cli"

**Escenario: El uso se desglosa por herramienta y subcomando**

Dado "claude-1" usó en "shop" 30 veces `status` por el MCP y 25 veces `raptor status --json`
Cuando el desarrollador consulta el desglose de "shop"
Entonces ve 30 en la herramienta `status` del canal mcp y 25 en el subcomando `raptor status` del canal cli

**Escenario: Una llamada se cuenta una sola vez**

Dado "claude-1" pide `status` por el MCP una vez
Cuando el desarrollador consulta el uso de "shop"
Entonces esa llamada cuenta 1 en el canal mcp y 0 en el canal cli

**Escenario: Lo que hace el desarrollador no cuenta**

Dado el desarrollador ejecuta `raptor status` 10 veces en "shop" desde su terminal
Cuando consulta el uso por canal de "shop"
Entonces esas 10 llamadas no figuran en el uso de agentes

**Escenario: El conteo no guarda lo que se pidió**

Dado "claude-1" pasó rutas y un mensaje de commit en sus llamadas a "shop"
Cuando el desarrollador consulta el uso por canal
Entonces ve conteos por canal, agente, repo, herramienta y resultado
  Y no ve rutas, mensajes ni parámetros de las llamadas

**Escenario: La medición no limita a ningún agente**

Dado "claude-1" ya hizo 500 llamadas por la CLI este mes en "shop"
Cuando hace una llamada más por la CLI
Entonces la llamada se atiende igual que las anteriores
  Y la consulta de uso no muestra ninguna meta

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), punto E.

- **Canal**: lo pone el daemon (`RequestChannel` del oplog, por el ejecutable del par), nunca el cliente. Solo cuentan `mcp` y `cli` con un agente como solicitante por ascendencia. El desarrollador y el hook no cuentan.
- **Sin contenido**: contadores agregados por (día, repo, agente, canal, etiqueta, resultado), sin parámetros, rutas, mensajes ni argv. ⚠️ **ASSUMPTION**: retención de 90 días. Sin meta y sin efecto en el rate limit.
- **Para la Dev Spec**: cómo se cuenta una invocación una sola vez. Recomendación: una etiqueta de enum cerrado por invocación, bajo una capacidad (ADR-GRP-016).

## Diseño y Dev Spec

- **Diseño:** no aplica (consulta por CLI; presentación en el Cockpit si se decide después).
- **Dev Spec:** **sí** (decisión del Arquitecto, 2026-11-10). Pendiente: `../dev-specs/US-MCP-025-dev-spec.md`.
