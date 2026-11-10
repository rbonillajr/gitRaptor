---
id: US-MCP-026
title: "El desarrollador compara, por tarea, cuánto cuesta y cuánto acierta un agente por el MCP y por la CLI"
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
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-020
    - US-MCP-025
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-010]
blocked_by: []
tags: [mcp, agentes, medicion, experimento, superficie-agentes, post-v0.1.0]
---

# US-MCP-026: El desarrollador compara, por tarea, cuánto cuesta y cuánto acierta un agente por el MCP y por la CLI

## Descripción

**Como** desarrollador orquestador, **quiero** comparar sobre las mismas tareas cuántas llamadas, cuántos tokens y cuántos aciertos tiene un agente que usa el MCP frente a uno que usa la guía y la CLI, **para** decidir con un dato propio qué camino recomiendo a mis agentes y si recorto herramientas del MCP.

**Valor**: US-MCP-025 cuenta llamadas, pero no tokens ni aciertos, que GitRaptor no ve. Sin esta comparación, la decisión de recortar el MCP se tomaría con el dato de Nx (88 % frente a 81 %), que es de terceros, no está verificado y se midió con otra herramienta.

**Prioridad e hito**: Could, después de v0.1.0. Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32). Si al refinarla no deja nada en el producto (solo un experimento y un informe), pasa a spike del Arquitecto (P-MCP-38).

**Sin dato** (P-MCP-38, Rene y el Arquitecto): cuántas ejecuciones por tarea forman la muestra, qué cuenta como acierto y de dónde salen los tokens (candidata: `/metrics` de nassa-core, a confirmar). La historia no está lista hasta tener esos tres datos.

## Reglas cubiertas

BR-MCP-CONS-010 (parte: tokens y acierto salen de un experimento aparte con fuente externa; el dato de Nx no es línea base) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-025 (llamadas por canal) y US-MCP-020 (la guía que usa el agente del camino CLI).
- **Externas**: fuente de tokens por tarea, sin confirmar.

## Criterios de Aceptación

**Escenario: La comparación da llamadas, tokens y aciertos por canal**

Dado un conjunto de tareas ejecutado por un agente con el MCP y por otro con la guía y la CLI, con la muestra acordada
Cuando el desarrollador consulta la comparación
Entonces ve, por tarea y por canal, las llamadas a GitRaptor, los tokens y si la tarea acertó según el criterio acordado

**Escenario: Cada cifra dice de dónde sale**

Dado la comparación de un conjunto de tareas
Cuando el desarrollador la consulta
Entonces las llamadas figuran como medidas por GitRaptor
  Y los tokens figuran con su fuente externa y la fecha de la medición

**Escenario: Con menos ejecuciones que la muestra no hay conclusión**

Dado un conjunto de tareas con menos ejecuciones que la muestra acordada en algún canal
Cuando el desarrollador consulta la comparación
Entonces ve las cifras con la marca "muestra insuficiente"
  Y no ve ninguna recomendación de canal

**Escenario: Sin la fuente de tokens, la comparación queda incompleta**

Dado un conjunto de tareas con la muestra acordada y la fuente externa de tokens no disponible
Cuando el desarrollador consulta la comparación
Entonces los tokens figuran como "sin dato"
  Y la comparación se marca como incompleta, sin recomendación de canal

**Escenario: El dato de terceros no se presenta como propio**

Dado la comparación de un conjunto de tareas
Cuando el desarrollador la consulta
Entonces no aparece el dato de Nx como línea base ni como meta

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), puntos B y G.

- **Datos de GitRaptor**: solo las llamadas por canal de US-MCP-025. Los tokens y el acierto salen de fuera.
- **Sirve a una decisión**: sus resultados alimentan el disparador de code mode (B, "cadenas largas"), hoy ⚠️ **ASSUMPTION**, y la decisión de recortar herramientas del MCP.

## Diseño y Dev Spec

- **Diseño:** no aplica (informe).
- **Dev Spec:** **no** (decisión del Arquitecto, 2026-11-10): es un experimento con fuente externa y pasa a spike si no deja nada en el producto (P-MCP-38). Sin muestra, criterio de acierto y fuente de tokens, no está lista.
